use core::borrow;
use std::time::Duration;

use async_graphql::{Enum, SimpleObject};
use shared::{
    connections::SteckerWebRTCConnection,
    models::{DataRoomInternalType, SteckerAudioChannel, SteckerData},
};
use tokio::sync::broadcast::Sender;
use uuid::Uuid;

// graphql objects

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
// #[graphql(remote = "shared::models::RoomType")]
pub enum RoomType {
    Float,
    Chat,
    Audio,
}

pub enum BroadcastRoom {
    Data(DataBroadcastRoom),
    Audio(AudioBroadcastRoom),
}

impl BroadcastRoom {
    pub fn meta(&self) -> &BroadcastRoomMeta {
        match self {
            BroadcastRoom::Data(data_room) => &data_room.meta,
            BroadcastRoom::Audio(audio_room) => &audio_room.meta,
        }
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        match self {
            BroadcastRoom::Data(data_room) => data_room.join_room(offer).await,
            BroadcastRoom::Audio(audio_room) => audio_room.join_room(offer).await,
        }
    }
}

struct BroadcastRoomMeta {
    pub name: String,
    pub uuid: Uuid,

    pub meta_reply: Sender<SteckerData>,
    pub meta_broadcast: Sender<SteckerData>,

    pub num_listeners: tokio::sync::watch::Sender<i64>,
    // we need to keep the channel open, so we attach
    // a receiver to the "lifetime" of this struct.
    // as receivers can be created from the sender,
    // this receiver does not need to be public accessible
    _num_listeners_receiver: tokio::sync::watch::Receiver<i64>,
}

impl From<RoomType> for DataRoomInternalType {
    fn from(value: RoomType) -> Self {
        match value {
            RoomType::Float => Self::Float,
            RoomType::Chat => Self::Chat,
            // @todo this is wrong!
            RoomType::Audio => Self::Chat,
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
pub struct DataBroadcastRoom {
    pub meta: BroadcastRoomMeta,
    /// Reply to server (messages not broadcasted)
    /// potentially not interesting to subscribe to this
    pub reply: Sender<SteckerData>,
    /// Subscribe to this to receive messages from room
    /// potentially not useful to send to this (unless you also become a broadcaster)
    pub broadcast: Sender<SteckerData>,
    pub room_type: DataRoomInternalType,
}

type ResponseOffer = String;

pub struct BroadcastRoomWithOffer {
    pub broadcast_room: DataBroadcastRoom,
    pub offer: ResponseOffer,
}

impl DataBroadcastRoom {
    pub async fn create_room(
        name: String,
        offer: String,
        room_type: DataRoomInternalType,
    ) -> anyhow::Result<BroadcastRoomWithOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer).await?;

        let stecker_data_channel = connection.register_channel(&room_type);
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);

        connection.start_listening_for_data_channel().await;

        let (num_listeners_sender, num_listeners_receiver) = tokio::sync::watch::channel(0);

        // thread for communication with creator
        // value-messages from creator are already handled via data_channel
        let mut num_listeners_receiver2 = num_listeners_receiver.clone();
        let mut close_receiver = stecker_data_channel.close.subscribe();
        let meta_outbound = meta_channel.outbound.clone();
        let mut meta_inbound = meta_channel.inbound.subscribe();
        let name2 = name.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = num_listeners_receiver2.changed() => {
                        let cur_num_listeners = *num_listeners_receiver2.borrow();
                        let msg = format!("Number of listeners @ {name2}: {cur_num_listeners}");
                        println!("{msg}");
                        let _ = meta_outbound.send(SteckerData::String(msg));
                    },
                    raw_meta_msg = meta_inbound.recv() => {
                        match raw_meta_msg {
                            Ok(meta_msg) => {
                                match meta_msg {
                                    SteckerData::String(msg) => {
                                        println!("Received meta_message form creator: {msg}");
                                    },
                                    _ => {println!("Received f32 from meta message?!");}
                                }

                            },
                            Err(err) => println!("Could not receive meta message from creator: {err}"),
                        }
                    },
                    _ = close_receiver.recv() => break,

                }
            }
        });

        let broadcast_room = DataBroadcastRoom {
            meta: BroadcastRoomMeta {
                name: name,
                uuid: Uuid::new_v4(),
                meta_broadcast: meta_channel.inbound.clone(),
                meta_reply: meta_channel.outbound.clone(),
                num_listeners: num_listeners_sender,
                _num_listeners_receiver: num_listeners_receiver,
            },
            room_type: room_type,
            reply: stecker_data_channel.outbound.clone(),
            broadcast: stecker_data_channel.inbound.clone(),
        };

        Ok(BroadcastRoomWithOffer {
            broadcast_room,
            offer: response_offer,
        })
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer.to_string()).await?;

        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let stecker_data_channel = connection.register_channel(&self.room_type.into());
        connection.start_listening_for_data_channel().await;

        let room_rx = self.broadcast.clone();
        let meta_rx = self.meta.meta_broadcast.clone();
        let close_trigger2 = stecker_data_channel.close.clone();

        let num_listeners2 = self.meta.num_listeners.clone();

        // incrementing num listeners needs to borrow the value to avoid deadlock
        let cur_num_listeners = *self.meta.num_listeners.borrow();
        let _ = self.meta.num_listeners.send(cur_num_listeners + 1);
        let mut num_listeners_receiver = self.meta.num_listeners.subscribe();

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
                            Ok(msg) => {println!("Broadcasting message from subscriber (will be ignored): {msg}");},
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
                            Ok(meta_msg) => println!("Meta message from subscriber (will be ignored): {meta_msg}"),
                            Err(err) => println!("Error on receiving inbound meta message: {err}"),
                        }
                    },
                    _ = num_listeners_receiver.changed() => {
                        let cur_num_listeners = *num_listeners_receiver.borrow();
                        let _ = meta_channel.outbound.send(SteckerData::String(format!("Number of listeners: {cur_num_listeners}").to_string()));
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

impl From<&BroadcastRoom> for Room {
    fn from(value: &BroadcastRoom) -> Self {
        let meta = value.meta();
        let room_type = match value {
            BroadcastRoom::Data(data_room) => data_room.room_type.into(),
            BroadcastRoom::Audio(_) => RoomType::Audio,
        };
        Room {
            uuid: meta.uuid.to_string(),
            name: meta.name.clone(),
            num_listeners: *meta.num_listeners.subscribe().borrow(),
            room_type,
        }
    }
}

pub struct AudioBroadcastRoom {
    pub meta: BroadcastRoomMeta,
    pub stecker_audio_channel: SteckerAudioChannel,
}

pub struct AudioBroadcastRoomWithOffer {
    pub audio_broadcast_room: AudioBroadcastRoom,
    pub offer: String,
}

impl AudioBroadcastRoom {
    pub async fn create_room(
        name: String,
        offer: String,
    ) -> anyhow::Result<AudioBroadcastRoomWithOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let audio_channel = connection.listen_for_audio_channel().await?;
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let (num_listeners_sender, num_listeners_receiver) = tokio::sync::watch::channel(0);

        let response_offer = connection.respond_to_offer(offer).await?;

        return Ok(AudioBroadcastRoomWithOffer {
            offer: response_offer,
            audio_broadcast_room: Self {
                stecker_audio_channel: audio_channel,
                meta: BroadcastRoomMeta {
                    name: name,
                    uuid: Uuid::new_v4(),
                    meta_broadcast: meta_channel.inbound.clone(),
                    meta_reply: meta_channel.outbound.clone(),
                    num_listeners: num_listeners_sender,
                    _num_listeners_receiver: num_listeners_receiver,
                },
            },
        });
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);

        let timeout = tokio::time::sleep(Duration::from_secs(10));
        let audio_track_receiver = self
            .stecker_audio_channel
            .audio_channel_tx
            .subscribe()
            .borrow()
            .clone();

        match audio_track_receiver {
            Some(audio_track) => {
                println!("Found an audio track :)");
                let _ = connection.add_existing_audio_track(audio_track).await;
                let response_offer = connection.respond_to_offer(offer.to_owned()).await?;
                Ok(response_offer)
            }
            None => Err(anyhow::anyhow!(
                "Have not received an audio track from the sender yet - try later"
            )),
        }
    }

    // pub async fn playback_room(offer: String) -> anyhow::Result<Self> {
    //     let connection = SteckerWebRTCConnection::build_connection().await?;
    //     let _ = connection.playback_audio_file().await.expect("No audio channel :l");
    //     let response_offer = connection.respond_to_offer(offer).await?;

    //     // let stecker_data_channel = connection.register_channel(&room_type);
    //     // let meta_channel = connection.register_channel(&SharedRoomType::Meta);

    //     Ok(AudioRoom {
    //         offer: response_offer
    //     })
    // }
}

impl Into<RoomType> for DataRoomInternalType {
    fn into(self) -> RoomType {
        match self {
            DataRoomInternalType::Float => RoomType::Float,
            DataRoomInternalType::Chat => RoomType::Chat,
            // @todo meta rooms do not exist exposed to the graphql api
            DataRoomInternalType::Meta => !unimplemented!(),
        }
    }
}
