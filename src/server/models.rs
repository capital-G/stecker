use async_graphql::{Enum, SimpleObject};
use shared::{
    connections::SteckerWebRTCConnection,
    models::{RoomType as SharedRoomType, SteckerSendable},
};
use tokio::sync::broadcast::Sender;
use uuid::Uuid;

// graphql objects

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
// #[graphql(remote = "shared::models::RoomType")]
pub enum RoomType {
    Float,
    Chat,
}

impl Into<SharedRoomType> for RoomType {
    fn into(self) -> SharedRoomType {
        match self {
            RoomType::Float => SharedRoomType::Float,
            RoomType::Chat => SharedRoomType::Chat,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct Room {
    pub uuid: String,
    pub name: String,
    pub num_listeners: i64,
    pub room_type: RoomType,
}

// server state objects
// #[derive(Clone)]
pub struct BroadcastRoom<T> {
    pub name: String,
    // Reply to server (messages not broadcasted)
    // potentially not interesting to subscribe to this
    pub reply: Sender<T>,
    // Subscribe to this to receive messages from room
    // potentially not useful to send to this (unless you also become a broadcaster)
    pub broadcast: Sender<T>,

    pub meta_reply: Sender<String>,
    pub meta_broadcast: Sender<String>,

    pub uuid: Uuid,
    pub room_type: SharedRoomType,

    pub num_listeners: tokio::sync::watch::Sender<i64>,
    // we need to keep the channel open, so we attach
    // a receiver to the "lifetime" of this struct.
    // as receivers can be created from the sender,
    // this receiver does not need to be public accessible
    _num_listeners_receiver: tokio::sync::watch::Receiver<i64>,
}

impl<T> From<&BroadcastRoom<T>> for Room {
    fn from(value: &BroadcastRoom<T>) -> Self {
        Room {
            uuid: value.uuid.to_string(),
            name: value.name.clone(),
            num_listeners: *value.num_listeners.subscribe().borrow(),
            room_type: value.room_type.into(),
        }
    }
}

type ResponseOffer = String;

pub struct BroadcastRoomWithOffer<T: Clone> {
    pub broadcast_room: BroadcastRoom<T>,
    pub offer: ResponseOffer,
}

impl<T: SteckerSendable> BroadcastRoom<T> {
    pub async fn create_room(
        name: String,
        offer: String,
        room_type: SharedRoomType,
    ) -> anyhow::Result<BroadcastRoomWithOffer<T>> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let meta_channel2 = connection.meta_channel.clone();
        let response_offer = connection.respond_to_offer(offer).await?;

        let stecker_data_channel = connection.create_data_channel::<T>(&room_type).await?;

        let (num_listeners_sender, num_listeners_receiver) = tokio::sync::watch::channel(0);

        // thread for communication with creator
        // value-messages from creator are already handled via data_channel
        let mut num_listeners_receiver2 = num_listeners_receiver.clone();
        let mut close_receiver = stecker_data_channel.close.subscribe();
        let meta_outbound = meta_channel2.outbound.clone();
        let mut meta_inbound = meta_channel2.inbound.subscribe();
        let name2 = name.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = num_listeners_receiver2.changed() => {
                        let cur_num_listeners = *num_listeners_receiver2.borrow();
                        let msg = format!("Number of listeners @ {name2}: {cur_num_listeners}");
                        println!("{msg}");
                        let _ = meta_outbound.send(msg);
                    },
                    raw_meta_msg = meta_inbound.recv() => {
                        match raw_meta_msg {
                            Ok(meta_msg) => println!("Received meta_message form creator: {meta_msg}"),
                            Err(err) => println!("Could not receive meta message from creator: {err}"),
                        }
                    },
                    _ = close_receiver.recv() => break,

                }
            }
        });

        let broadcast_room = BroadcastRoom {
            name: name,
            reply: stecker_data_channel.outbound.clone(),
            broadcast: stecker_data_channel.inbound,
            uuid: Uuid::new_v4(),
            room_type: room_type,
            meta_broadcast: meta_channel2.inbound,
            meta_reply: meta_channel2.outbound,
            num_listeners: num_listeners_sender,
            _num_listeners_receiver: num_listeners_receiver,
        };

        Ok(BroadcastRoomWithOffer {
            broadcast_room,
            offer: response_offer,
        })
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer.to_string()).await?;
        let meta_channel = connection.meta_channel.clone();

        // let stecker_data_channel = connection
        //     .listen_for_data_channel::<T>(&self.room_type.into())
        //     .await;

        let stecker_data_channel = connection.listen_for_data_channel::<T>(&self.room_type.into()).await;

        let room_rx = self.broadcast.clone();
        let meta_rx = self.meta_broadcast.clone();
        let close_trigger2 = stecker_data_channel.close.clone();

        let num_listeners2 = self.num_listeners.clone();

        // incrementing num listeners needs to borrow the value to avoid deadlock
        let cur_num_listeners = *self.num_listeners.borrow();
        let _ = self.num_listeners.send(cur_num_listeners + 1);
        let mut num_listeners_receiver = self.num_listeners.subscribe();

        tokio::spawn(async move {
            let mut room_receiver = room_rx.subscribe();
            let mut meta_receiver = meta_rx.subscribe();
            let mut stop_receiver = stecker_data_channel.close.subscribe();

            // Listen to room messages and pass them to client
            let mut inbound_receiver = stecker_data_channel.inbound.subscribe();
            let mut meta_inbound_receiver = meta_channel.inbound.subscribe();
            let mut stop_receiver2 = close_trigger2.subscribe();

            loop {
                tokio::select! {
                    raw_msg = room_receiver.recv() => {
                        match raw_msg {
                            Ok(msg) => {
                                let _ = stecker_data_channel.outbound.send(msg);
                            },
                            Err(err) => {
                                while room_receiver.len() > 0 {
                                    let _ = room_receiver.recv().await;
                                }
                                println!("Got some lagging problems: {err}");
                            },
                        }
                    },
                    raw_msg = inbound_receiver.recv() => {
                        match raw_msg {
                            Ok(msg) => {println!("Broadcasting message from subscriber - will be ignored {msg}");},
                            Err(err) => {println!("Got errors when receiving inbound message: {err}")},
                        }
                    },
                    raw_meta_msg = meta_receiver.recv() => {
                        match raw_meta_msg {
                            Ok(meta_msg) => {
                                let _ = meta_channel.outbound.send(meta_msg);
                            },
                            Err(err) => {println!("Error forwarding meta message: {err}")},
                        }
                    },
                    raw_msg = meta_inbound_receiver.recv() => {
                        match raw_msg {
                            Ok(meta_msg) => println!("Meta message from subscriber - will be ignored {meta_msg}"),
                            Err(err) => println!("Error on receiving inbound meta message: {err}"),
                        }
                    },
                    _ = num_listeners_receiver.changed() => {
                        let cur_num_listeners = *num_listeners_receiver.borrow();
                        let _ = meta_channel.outbound.send(format!("Number of listeners: {cur_num_listeners}").to_string());
                    },
                    _ = stop_receiver2.recv() => {
                        println!("Got triggered and stop consuming inbound messages now");
                        break
                    },
                    _ = stop_receiver.recv() => {
                        println!("Received a stop signal");
                        break
                    }
                };
            }
            let cur_num_listeners = *num_listeners2.borrow();
            let _ = num_listeners2.send(cur_num_listeners - 1);
        });
        Ok(response_offer)
    }
}

pub enum AnyBroadcastRoom {
    FloatBroadcastRoom(BroadcastRoom<f32>),
    ChatBroadcastRoom(BroadcastRoom<String>),
}

impl Into<RoomType> for SharedRoomType {
    fn into(self) -> RoomType {
        match self {
            SharedRoomType::Float => RoomType::Float,
            SharedRoomType::Chat => RoomType::Chat,
            // meta rooms do not exist exposed to the graphql api
            SharedRoomType::Meta => !unimplemented!(),
        }
    }
}
