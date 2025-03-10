use std::{any, fmt::Display, sync::Arc, time::Duration};

use anyhow::anyhow;
use async_graphql::{Enum, SimpleObject};
use shared::{
    connections::SteckerWebRTCConnection,
    models::{DataRoomInternalType, SteckerAudioChannel, SteckerData},
};
use tokio::sync::broadcast::Sender;
use tracing::{error, info, instrument, trace, warn, Instrument, Span};
use uuid::Uuid;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

// graphql objects

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
// #[graphql(remote = "shared::models::RoomType")]
pub enum RoomType {
    Float,
    Chat,
    Audio,
}

impl Display for RoomType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            RoomType::Float => write!(f, "FloatRoom"),
            RoomType::Chat => write!(f, "ChatRoom"),
            RoomType::Audio => write!(f, "AudioRoom"),
        }
    }
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

    /// replace sender of current broadcast
    pub async fn replace_sender(
        &self,
        offer: &str,
        password: &str,
    ) -> anyhow::Result<ResponseOffer> {
        match self {
            BroadcastRoom::Audio(audio_room) => audio_room.replace_sender(offer.to_string()).await,
            BroadcastRoom::Data(data_broadcast_room) => todo!(),
        }
    }
}

pub struct BroadcastRoomMeta {
    pub name: String,
    pub uuid: Uuid,
    pub admin_password: String,

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
    #[instrument(skip_all, err)]
    pub async fn create_room(
        name: String,
        offer: String,
        room_type: DataRoomInternalType,
        password: String,
    ) -> anyhow::Result<BroadcastRoomWithOffer> {
        info!("Something else");
        let connection = SteckerWebRTCConnection::build_connection()
            .instrument(Span::current())
            .await?;
        let response_offer = connection
            .respond_to_offer(offer)
            .instrument(Span::current())
            .await?;

        let stecker_data_channel = connection.register_channel(&room_type);
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);

        connection
            .start_listening_for_data_channel()
            .instrument(Span::current())
            .await;

        let (num_listeners_sender, num_listeners_receiver) = tokio::sync::watch::channel(0);

        // thread for communication with creator
        // value-messages from creator are already handled via data_channel
        let mut num_listeners_receiver2 = num_listeners_receiver.clone();
        let mut close_receiver = stecker_data_channel.close.subscribe();
        let meta_outbound = meta_channel.outbound.clone();
        let mut meta_inbound = meta_channel.inbound.subscribe();
        let name2 = name.clone();
        tokio::spawn(
            async move {
                loop {
                    tokio::select! {
                        _ = num_listeners_receiver2.changed() => {
                            let cur_num_listeners = *num_listeners_receiver2.borrow();
                            info!(cur_num_listeners, "Changed number of listeners");
                            let _ = meta_outbound.send(SteckerData::String(
                                format!("Number of listeners @ {name2}: {cur_num_listeners}")
                            ));
                        },
                        raw_meta_msg = meta_inbound.recv() => {
                            match raw_meta_msg {
                                Ok(meta_msg) => {
                                    match meta_msg {
                                        SteckerData::String(msg) => {
                                            trace!(msg, "Received meta_message form creator");
                                        },
                                        _ => {error!("Received f32 from meta message?!");}
                                    }

                                },
                                Err(_) => {
                                    error!("Could not receive meta message from creator");
                                },
                            }
                        },
                        _ = close_receiver.recv() => break,

                    }
                }
            }
            .instrument(Span::current()),
        );

        let broadcast_room = DataBroadcastRoom {
            meta: BroadcastRoomMeta {
                name: name,
                uuid: Uuid::new_v4(),
                meta_broadcast: meta_channel.inbound.clone(),
                meta_reply: meta_channel.outbound.clone(),
                num_listeners: num_listeners_sender,
                _num_listeners_receiver: num_listeners_receiver,
                admin_password: password,
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

    #[instrument(skip_all, err)]
    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer.to_string()).await?;

        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let stecker_data_channel = connection.register_channel(&self.room_type.into());
        connection
            .start_listening_for_data_channel()
            .instrument(Span::current())
            .await;

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
                                match err {
                                    tokio::sync::broadcast::error::RecvError::Closed => error!("Channel is already closed"),
                                    tokio::sync::broadcast::error::RecvError::Lagged(lag) => warn!(lag, "Lagging behind"),
                                }
                            },
                        }
                    },
                    raw_msg = inbound_receiver.recv() => {
                        match raw_msg {
                            Ok(msg) => warn!(?msg, "Broadcasting message from subscriber will be ignored"),
                            Err(_) => error!("Error while receiving inbound message"),
                        }
                    },
                    raw_meta_msg = meta_receiver.recv() => {
                        match raw_meta_msg {
                            Ok(meta_msg) => {
                                trace!(?meta_msg, "Send out meta message");
                                let _ = meta_channel.outbound.send(meta_msg);
                            },
                            Err(_) => {error!("Failed to forward meta message")},
                        }
                    },
                    raw_msg = meta_inbound_receiver.recv() => {
                        match raw_msg {
                            Ok(meta_msg) => warn!(?meta_msg, "Meta message from subscriber will be ignored"),
                            Err(_) => error!("Error on receiving inbound meta message"),
                        }
                    },
                    _ = num_listeners_receiver.changed() => {
                        let cur_num_listeners = *num_listeners_receiver.borrow();
                        info!(cur_num_listeners, "Number of listeners changed");
                        let _ = meta_channel.outbound.send(SteckerData::String(format!("Number of listeners: {cur_num_listeners}").to_string()));
                    },
                    _ = stop_receiver2.recv() => {
                        trace!("Stop consuming inbound messages now");
                        break
                    },
                    _ = stop_receiver.recv() => {
                        trace!("Received stop signal");
                        break
                    }
                };
            }
            let cur_num_listeners = *num_listeners2.borrow();
            let _ = num_listeners2.send(cur_num_listeners - 1);
        }.instrument(Span::current()));
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
    pub disconnected: Sender<()>,
}

impl AudioBroadcastRoom {
    pub async fn create_room(
        name: String,
        offer: String,
        admin_password: String,
    ) -> anyhow::Result<AudioBroadcastRoomWithOffer> {
        let connection = SteckerWebRTCConnection::build_connection()
            .in_current_span()
            .await?;
        // let audio_channel = connection.listen_for_audio_channel().await?;
        let audio_channel = SteckerAudioChannel::create_channels();
        let mut audio_track_receiver = connection
            .listen_for_remote_audio_track()
            .in_current_span()
            .await;
        let meta_channel = connection.register_channel(&DataRoomInternalType::Meta);
        let (num_listeners_sender, num_listeners_receiver) = tokio::sync::watch::channel(0);
        let response_offer = connection.respond_to_offer(offer).in_current_span().await?;

        let audio_channel_tx = audio_channel.audio_channel_tx.clone();
        let disconnected = connection.connection_closed.clone();
        let mut stop_consuming = audio_channel.reset_sender.subscribe();
        let seq_number_sender = audio_channel.sequence_number_sender.clone();

        // a thread which consumes the audio data we receive and pushes it to our internal
        // webrtc channel which is then read/consumed and pushed to all our subscribers
        tokio::spawn(
            async move {
                let track = audio_track_receiver.recv().await.unwrap();
                let local_track = Arc::new(TrackLocalStaticRTP::new(
                    track.codec().capability,
                    "audio".to_owned(),
                    "stecker".to_owned(),
                ));

                let _ = audio_channel_tx.send(Some(local_track.clone()));

                loop {
                    tokio::select! {
                        result = track.read_rtp() => {
                            if let Ok((rtp, _)) = result {
                                let seq_number = rtp.header.sequence_number;
                                let _ = seq_number_sender.send(seq_number);
                                // trace!(seq_number, "Currently sending");
                                let _ = local_track.write_rtp(&rtp).await;
                            } else {
                                error!("Failed to read track - stop consuming");
                                break;
                            }
                       },
                       _ = stop_consuming.recv() => {
                            info!("Got signal to terminate consuming the current track");
                            break;
                       }
                    }
                }
            }
            .in_current_span(),
        );

        return Ok(AudioBroadcastRoomWithOffer {
            offer: response_offer,
            disconnected,
            audio_broadcast_room: Self {
                stecker_audio_channel: audio_channel,
                meta: BroadcastRoomMeta {
                    name: name,
                    uuid: Uuid::new_v4(),
                    meta_broadcast: meta_channel.inbound.clone(),
                    meta_reply: meta_channel.outbound.clone(),
                    num_listeners: num_listeners_sender,
                    _num_listeners_receiver: num_listeners_receiver,
                    admin_password,
                },
            },
        });
    }

    pub async fn join_room(&self, offer: &str) -> anyhow::Result<ResponseOffer> {
        let connection = SteckerWebRTCConnection::build_connection().await?;
        let _meta_channel = connection.register_channel(&DataRoomInternalType::Meta);

        let audio_track_receiver = self
            .stecker_audio_channel
            .audio_channel_tx
            .subscribe()
            .borrow()
            .clone();

        match audio_track_receiver {
            Some(audio_track) => {
                trace!("Found an audio track");
                let _ = connection.add_existing_audio_track(audio_track).await;
                let response_offer = connection.respond_to_offer(offer.to_owned()).await?;
                Ok(response_offer)
            }
            None => Err(anyhow::anyhow!(
                "Have not received an audio track from the sender yet - try later"
            )),
        }
    }

    pub async fn replace_sender(&self, offer: String) -> anyhow::Result<ResponseOffer> {
        info!("Replace audio sender");
        let connection = SteckerWebRTCConnection::build_connection()
            .in_current_span()
            .await?;
        let response_offer = connection.respond_to_offer(offer).in_current_span().await?;
        let mut audio_track_receiver = connection
            .listen_for_remote_audio_track()
            .in_current_span()
            .await;

        let local_track = if let Some(track) = self
            .stecker_audio_channel
            .audio_channel_tx
            .subscribe()
            .borrow()
            .clone()
        {
            track
        } else {
            return Err(anyhow::anyhow!(
                "Room has not been sucessfully set up, can not take it over."
            ));
        };

        let _ = self.stecker_audio_channel.reset_sender.send(());

        let mut stop_consuming = self.stecker_audio_channel.reset_sender.subscribe();
        let seq_number_sender = self.stecker_audio_channel.sequence_number_sender.clone();
        let mut seq_number_receiver = self.stecker_audio_channel.sequence_number_receiver.clone();
        let _ = *seq_number_receiver.borrow_and_update();

        tokio::spawn(
            async move {
                let track = audio_track_receiver.recv().await.unwrap();
                let ssrc = track.ssrc();
                trace!(ssrc, "Start consuming new audio track");

                let mut last_seq: u16 = (*seq_number_receiver.borrow_and_update()).clone();

                loop {
                    tokio::select! {
                        result = track.read_rtp() => {
                            if let Ok((mut rtp, _)) = result {
                                // we need to reorder RTP packages b/c otherwise the client will
                                // think there was a package drop b/c of a gap in the seq order
                                last_seq = last_seq.wrapping_add(1);
                                rtp.header.sequence_number = last_seq;
                                let _ = seq_number_sender.send(last_seq);
                                let _ = local_track.write_rtp(&rtp).await;
                            } else {
                                error!("Failed to read track - stop consuming");
                                break;
                            }
                       },
                       _ = stop_consuming.recv() => {
                            info!("Got signal to terminate consuming the current track");
                            break;
                        }
                    }
                }
            }
            .in_current_span(),
        );

        Ok(response_offer)
    }
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

#[derive(SimpleObject, Clone)]
pub struct RoomCreationReply {
    pub offer: String,
    pub password: String,
}
