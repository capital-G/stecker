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

#[derive(SimpleObject)]
pub struct Room {
    pub uuid: String,
    pub name: String,
    pub num_listeners: usize,
    pub room_type: RoomType,
}

// server state objects
#[derive(Clone)]
pub struct BroadcastRoom<T: Clone> {
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
        let response_offer = connection.respond_to_offer(offer).await?;

        let meta_channel = connection
            .listen_for_data_channel::<String>(&SharedRoomType::Meta)
            .await;

        let stecker_data_channel = connection.listen_for_data_channel::<T>(&room_type).await;

        let broadcast_room = BroadcastRoom {
            name: name,
            reply: stecker_data_channel.outbound.clone(),
            broadcast: stecker_data_channel.inbound,
            uuid: Uuid::new_v4(),
            room_type: room_type,
            meta_broadcast: meta_channel.inbound,
            meta_reply: meta_channel.outbound.clone(),
        };

        Ok(BroadcastRoomWithOffer {
            broadcast_room,
            offer: response_offer,
        })
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer.to_string()).await?;

        let meta_channel = connection
            .listen_for_data_channel::<String>(&SharedRoomType::Meta)
            .await;

        let stecker_data_channel = connection
            .listen_for_data_channel::<T>(&self.room_type.into())
            .await;

        let room_rx = self.broadcast.clone();
        let close_trigger2 = stecker_data_channel.close_trigger.clone();

        tokio::spawn(async move {
            //
            let mut room_receiver = room_rx.subscribe();
            let mut stop_receiver = stecker_data_channel.close_trigger.subscribe();
            let mut consume = true;

            // Listen to room messages and pass them to client
            let mut inbound_receiver = stecker_data_channel.inbound.subscribe();
            let mut stop_receiver2 = close_trigger2.subscribe();

            // meta channel
            let mut meta_inbound = meta_channel.inbound.subscribe();
            let mut meta_outbound = meta_channel.outbound.subscribe();

            while consume {
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
                    _ = stop_receiver2.recv() => {
                        println!("Got triggered and stop consuming inbound messages now");
                        consume = false;
                    },
                    _ = stop_receiver.recv() => {
                        println!("Received a stop signal");
                        consume = false;
                    }
                };
            }
        });
        Ok(response_offer)
    }
}

pub enum AnyBroadcastRoom {
    FloatBroadcastRoom(BroadcastRoom<f32>),
    ChatBroadcastRoom(BroadcastRoom<String>),
}

impl<T: Clone> From<BroadcastRoom<T>> for Room {
    fn from(value: BroadcastRoom<T>) -> Self {
        Self {
            uuid: value.uuid.to_string(),
            name: value.name.to_string(),
            num_listeners: 0,
            room_type: value.room_type.into(),
        }
    }
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
