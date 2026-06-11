use crate::event_service::RoomEvent;
use anyhow::anyhow;
use async_graphql::{Enum, SimpleObject};
use shared::connections::SteckerWebRTCConnection;
use shared::models::{
    DataChannelEvent, RoomFloatData, RoomStringData, SteckerAudioChannel, SteckerData,
    SteckerDataChanelTrait, SteckerDataChannel,
};
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::sync::{broadcast, watch, Mutex, RwLock};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace, Instrument};
use uuid::Uuid;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

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

// an abstraction for generic data channel creation
#[derive(Clone, Copy, Debug, Enum, PartialEq, Eq)]
pub enum DataChannelKind {
    Float,
    String,
}

pub trait ChannelAccess<T: SteckerData> {
    fn get_field(room: &BroadcastRoom) -> &RwLock<Option<Arc<SteckerDataChannel<T>>>>;
}

impl ChannelAccess<RoomFloatData> for DataChannelKind {
    fn get_field(room: &BroadcastRoom) -> &RwLock<Option<Arc<SteckerDataChannel<RoomFloatData>>>> {
        &room.float_channel
    }
}

impl ChannelAccess<RoomStringData> for DataChannelKind {
    fn get_field(room: &BroadcastRoom) -> &RwLock<Option<Arc<SteckerDataChannel<RoomStringData>>>> {
        &room.chat_channel
    }
}

#[derive(Debug)]
pub struct BroadcastRoom {
    audio_channel: RwLock<Option<Arc<SteckerAudioChannel>>>,
    float_channel: RwLock<Option<Arc<SteckerDataChannel<RoomFloatData>>>>,
    chat_channel: RwLock<Option<Arc<SteckerDataChannel<RoomStringData>>>>,
    /// if a stream gets taken over we must re-assign the audio_sequence_number b/c
    /// otherwhise the stream will think it has stalled, which will result in silence
    audio_sequence_offset: watch::Sender<u16>,

    // all metadata for a room is stored in a dedicated such that it can be cloned for schema access
    meta: BroadcastRoomMeta,

    pub free_room: CancellationToken,
    active_channels: Arc<Mutex<u32>>,
    current_deletion_token: Arc<Mutex<CancellationToken>>,
    timeout: Duration,
}

type ResponseOffer = String;

impl BroadcastRoom {
    pub fn new(name: String, password: String, uuid: Uuid, description: String) -> Self {
        let (audio_sequence_offset, _) = watch::channel(0);
        Self {
            audio_channel: RwLock::new(None),
            float_channel: RwLock::new(None),
            chat_channel: RwLock::new(None),
            meta: BroadcastRoomMeta::new(name.clone(), uuid, password.clone(), description),
            current_deletion_token: Arc::new(Mutex::new(CancellationToken::new())),
            free_room: CancellationToken::new(),
            active_channels: Arc::new(Mutex::new(0)),
            timeout: Duration::from_secs(30),
            audio_sequence_offset,
        }
    }

    pub async fn get_current_channel_types(&self) -> Vec<ChannelKind> {
        let mut channels = vec![];
        if self.audio_channel.read().await.is_some() {
            channels.push(ChannelKind::AudioChannel);
        }
        if self.chat_channel.read().await.is_some() {
            channels.push(ChannelKind::DataChannel(DataChannelKind::String));
        }
        if self.float_channel.read().await.is_some() {
            channels.push(ChannelKind::DataChannel(DataChannelKind::Float));
        }
        channels
    }

    pub fn meta(&self) -> &BroadcastRoomMeta {
        &self.meta
    }

    /// replace sender of current broadcast
    pub async fn replace_sender(
        self: &Arc<Self>,
        kind: ChannelKind,
        offer: String,
        password: Option<String>,
        room_events: tokio::sync::broadcast::Sender<RoomEvent>,
    ) -> anyhow::Result<ResponseOffer> {
        match password {
            Some(ref pw) if pw == &self.meta.admin_password => {}
            Some(_) => return Err(anyhow!("Invalid password")),
            None => return Err(anyhow!("Password required to replace sender")),
        }

        match kind {
            ChannelKind::AudioChannel => self.replace_audio_sender(offer, room_events).await,
            ChannelKind::DataChannel(_) => Err(anyhow!(
                "Replacing data channel senders is not yet supported"
            )),
        }
    }

    #[instrument(skip_all)]
    async fn replace_audio_sender(
        self: &Arc<Self>,
        offer: String,
        room_events: tokio::sync::broadcast::Sender<RoomEvent>,
    ) -> anyhow::Result<ResponseOffer> {
        let audio_channel = {
            let guard = self.audio_channel.read().await;
            match &*guard {
                Some(channel) => channel.clone(),
                None => return Err(anyhow!("No audio channel to replace")),
            }
        };

        audio_channel.reset_sender.notify_waiters();

        let (connection_events, _) = broadcast::channel(256);
        let connection = Arc::new(
            SteckerWebRTCConnection::build_connection(connection_events)
                .in_current_span()
                .await?,
        );
        let offer = connection.respond_to_offer(&offer).await?;

        let room = self.clone();

        tokio::spawn(
            async move {
                room.current_deletion_token.lock().await.cancel();
                *room.active_channels.lock().await += 1;

                let audio_track_reply = tokio::select! {
                    remote_track = connection.wait_for_audio_channel() => {
                        debug!("Received replacement audio channel");
                        Some(remote_track)
                    },
                    _ = sleep(Duration::from_secs(30)) => {
                        info!("Replacement sender timed out");
                        None
                    },
                    _ = connection.wait_for_disconnect() => {
                        info!("Replacement sender disconnected before providing audio");
                        None
                    }
                };

                if let Some(audio_track) = audio_track_reply {
                    let local_track = {
                        let existing = audio_channel.audio_channel_rx.borrow();
                        match &*existing {
                            Some(track) => track.clone(),
                            None => Arc::new(TrackLocalStaticRTP::new(
                                audio_track.codec().capability,
                                "audio".to_string(),
                                "stecker".to_string(),
                            )),
                        }
                    };
                    let _ =
                        room_events.send(RoomEvent::BroadcastRoomUpdated(room.meta.name.clone()));

                    let reset_notify = audio_channel.reset_sender.clone();
                    let offset = *room.audio_sequence_offset.borrow();
                    loop {
                        tokio::select! {
                            Ok((mut rtp, _)) = audio_track.read_rtp() => {
                                let mut seq_number = rtp.header.sequence_number;
                                seq_number = seq_number.wrapping_add(offset);
                                let _ = room.audio_sequence_offset.send(seq_number);
                                rtp.header.sequence_number = seq_number;
                                let _ = local_track.write_rtp(&rtp).await;
                            },
                            _ = connection.wait_for_disconnect() => break,
                            _ = reset_notify.notified() => {
                                info!("Sender replaced again");
                                let _ = connection.close().await;
                                *room.active_channels.lock().await -= 1;
                                return;
                            }
                            else => break,
                        }
                    }
                }

                let _ = connection.close().await;
                trace!("Release audio channel");
                *room.audio_channel.write().await = None;

                Self::check_deletion(
                    room.active_channels.clone(),
                    room.current_deletion_token.clone(),
                    room.timeout,
                    room.free_room.clone(),
                )
                .await;
            }
            .in_current_span(),
        );

        Ok(offer)
    }

    // checks if a deletion is necessary - this can be cancelled
    async fn check_deletion(
        active_channels: Arc<Mutex<u32>>,
        room_deletion_token: Arc<Mutex<CancellationToken>>,
        room_timeout: Duration,
        trigger_free_room: CancellationToken,
    ) {
        *active_channels.lock().await -= 1;

        if (*active_channels.lock().await) > 0 {
            return;
        }

        let new_cancellation_token = CancellationToken::new();
        let cancel_token = new_cancellation_token.clone();
        {
            let mut active = room_deletion_token.lock().await;
            *active = new_cancellation_token;
        }

        trace!(
            ?room_timeout,
            "Room will be freed if no new channel will be created"
        );

        tokio::select! {
            _ = cancel_token.cancelled() => {
                trace!("Room timeout got cancelled");
            }
            _ = sleep(room_timeout) => {
                trace!("Room can be freed");
                let _ = trigger_free_room.cancel();
            }
        }
    }

    #[instrument(skip_all)]
    pub async fn create_data_channel<T>(
        self: &Arc<Self>,
        offer: &String,
        kind: DataChannelKind,
    ) -> anyhow::Result<ResponseOffer>
    where
        T: SteckerData + 'static + Clone,
        DataChannelKind: ChannelAccess<T>,
    {
        info!(?kind, "Creating data channel");
        let (connection_events, _) = broadcast::channel(256);
        let connection = Arc::new(
            SteckerWebRTCConnection::build_connection(connection_events)
                .in_current_span()
                .await?,
        );
        let offer = connection.respond_to_offer(offer).await?;

        let data_channel = Arc::new(SteckerDataChannel::<T>::create_channels());
        let data_channel_clone = data_channel.clone();
        let mut channel_events = data_channel.events.clone().subscribe();
        *DataChannelKind::get_field(self).write().await = Some(data_channel);

        let room = self.clone();
        tokio::spawn(async move {
            let mut active_timeout = true;
            room.current_deletion_token.lock().await.cancel();
            *room.active_channels.lock().await += 1;
            loop {
                tokio::select! {
                    rtc_connection = connection.wait_for_data_channel::<T>() => {
                        debug!("Found matching data channel");
                        let _ = data_channel_clone.connect(&rtc_connection).in_current_span().await;
                        active_timeout = false;
                    },
                    _ = sleep(Duration::from_secs(20)) => {
                        if active_timeout {
                            info!("Time out - stop listening for connection");
                            break;
                        }
                    },
                    Ok(DataChannelEvent::ClosedConnection) = channel_events.recv() => break,
                }
            }
            let _ = connection.close().await;
            trace!("Release data channel");
            *DataChannelKind::get_field(&room).write().await = None;

            Self::check_deletion(
                room.active_channels.clone(),
                room.current_deletion_token.clone(),
                room.timeout,
                room.free_room.clone(),
            ).await;
        }.in_current_span());

        Ok(offer)
    }

    #[instrument(skip(self, offer))]
    pub async fn join_data_channel<T>(
        &self,
        offer: &String,
        kind: DataChannelKind,
    ) -> anyhow::Result<ResponseOffer>
    where
        T: SteckerData + 'static + Clone,
        DataChannelKind: ChannelAccess<T>,
    {
        info!("Joining data channel");
        let (connection_events, _) = broadcast::channel(256);
        let connection = Arc::new(
            SteckerWebRTCConnection::build_connection(connection_events)
                .in_current_span()
                .await?,
        );
        let offer = connection.respond_to_offer(offer).await?;

        let client_data_channel = Arc::new(SteckerDataChannel::<T>::create_channels());

        let data_channel_handle = DataChannelKind::get_field(&self);

        let guard = data_channel_handle.read().await;
        match &*guard {
            Some(server_data_channel) => {
                let mut server_channel_messages = server_data_channel.inbound.subscribe();
                let mut server_channel_events = server_data_channel.events.subscribe();
                let client_channel_messages = client_data_channel.outbound.clone();
                let mut client_channel_events = client_data_channel.events.subscribe();

                tokio::spawn(async move {
                    let success_connection = tokio::select! {
                        _ = sleep(Duration::from_secs(30)) => {
                            trace!("Timed out!");
                            false
                        },
                        rtc_connection = connection.wait_for_data_channel::<T>() => {
                            let _ = client_data_channel.connect(&rtc_connection).await;
                            true
                        },
                        _ = connection.wait_for_disconnect() => {
                            trace!("Connection got closed while waiting for data channel");
                            false
                        }
                    };

                    if success_connection {
                        loop {
                            tokio::select! {
                                Ok(inbound_message) = server_channel_messages.recv() => {
                                    let _ = client_channel_messages.send(inbound_message);
                                },
                                Ok(DataChannelEvent::ClosedConnection) = server_channel_events.recv() => break,
                                Ok(DataChannelEvent::ClosedConnection) = client_channel_events.recv() => break,
                            }
                        }
                    }
                    let _ = connection.close().await;
                }.in_current_span());
                Ok(offer)
            }
            None => Err(anyhow!("Could not find requested data channel")),
        }
    }

    #[instrument(skip_all)]
    pub async fn create_audio_channel(
        self: &Arc<Self>,
        offer: &String,
        room_events: tokio::sync::broadcast::Sender<RoomEvent>,
    ) -> anyhow::Result<ResponseOffer> {
        info!("Creating audio channel");
        let (connection_events, _) = broadcast::channel(256);
        let connection = Arc::new(
            SteckerWebRTCConnection::build_connection(connection_events)
                .in_current_span()
                .await?,
        );
        let offer = connection.respond_to_offer(offer).await?;

        let audio_channel = Arc::new(SteckerAudioChannel::create_channels());
        let audio_channel_clone = audio_channel.clone();
        *self.audio_channel.write().await = Some(audio_channel);

        let room = self.clone();

        tokio::spawn(
            async move {
                room.current_deletion_token.lock().await.cancel();
                *room.active_channels.lock().await += 1;
                let reset_notify = audio_channel_clone.reset_sender.clone();
                let mut replaced = false;

                let audio_track_reply = tokio::select! {
                    remote_track = connection.wait_for_audio_channel() => {
                        debug!("Received audio channel from other side");
                        Some(remote_track)
                    },
                    _ = sleep(Duration::from_secs(30)) => {
                        info!("Other side failed to provide audio channel - stop listening.");
                        None
                    },
                    _ = connection.wait_for_disconnect() => {
                        info!("Disconnected while waiting for audio channel");
                        None
                    }
                };

                if let Some(audio_track) = audio_track_reply {
                    let local_track = Arc::new(TrackLocalStaticRTP::new(
                        audio_track.codec().capability,
                        "audio".to_string(),
                        "stecker".to_string(),
                    ));
                    let _ = audio_channel_clone
                        .audio_channel_tx
                        .send(Some(local_track.clone()));
                    let _ =
                        room_events.send(RoomEvent::BroadcastRoomStreaming(room.meta.name.clone()));

                    let offset = *room.audio_sequence_offset.borrow();
                    loop {
                        tokio::select! {
                            Ok((mut rtp, _)) = audio_track.read_rtp() => {
                                let mut seq_number = rtp.header.sequence_number;
                                seq_number = seq_number.wrapping_add(offset);
                                let _ = room.audio_sequence_offset.send(seq_number);
                                rtp.header.sequence_number = seq_number;
                                let _ = local_track.write_rtp(&rtp).await;
                            },
                            _ = connection.wait_for_disconnect() => break,
                            _ = reset_notify.notified() => {
                                info!("Sender replaced");
                                replaced = true;
                                break;
                            }
                            else => {
                                error!("Error while consuming audio track - bail out");
                                break;
                            }
                        }
                    }
                }

                let _ = connection.close().await;

                if replaced {
                    *room.active_channels.lock().await -= 1;
                } else {
                    trace!("Release audio channel");
                    *room.audio_channel.write().await = None;

                    Self::check_deletion(
                        room.active_channels.clone(),
                        room.current_deletion_token.clone(),
                        room.timeout,
                        room.free_room.clone(),
                    )
                    .await;
                }
            }
            .in_current_span(),
        );

        Ok(offer)
    }

    #[instrument(skip_all)]
    pub async fn join_audio_channel(&self, offer: &String) -> anyhow::Result<ResponseOffer> {
        info!("Joining audio channel");
        let (connection_events, _) = broadcast::channel(256);
        let connection = Arc::new(
            SteckerWebRTCConnection::build_connection(connection_events)
                .in_current_span()
                .await?,
        );

        let audio_track_opt = {
            let guard = self.audio_channel.read().await;
            match &*guard {
                Some(audio_channel) => {
                    let watch = audio_channel.audio_channel_rx.borrow();
                    watch.clone()
                }
                None => {
                    anyhow::bail!("Could not find audio channel for room");
                }
            }
        };

        match audio_track_opt {
            Some(audio_track) => {
                connection.add_existing_audio_track(audio_track).await;
            }
            None => {
                connection.close().await?;
                return Err(anyhow::anyhow!("Sender did not send audio track yet"));
            }
        }

        let offer = connection.respond_to_offer(offer).await?;

        let c = connection.clone();
        tokio::spawn(
            async move {
                let _ = c.clone().wait_for_disconnect().await;
                let _ = c.close().await;
            }
            .in_current_span(),
        );

        Ok(offer)
    }
}

#[derive(Debug)]
pub struct BroadcastRoomMeta {
    pub name: String,
    pub uuid: Uuid,
    pub admin_password: String,
    pub description: String,

    // pub meta_reply: Sender<SteckerData>,
    // pub meta_broadcast: Sender<SteckerData>,
    pub num_listeners: tokio::sync::watch::Sender<i32>,
    // we need to keep the channel open, so we attach
    // a receiver to the "lifetime" of this struct.
    // as receivers can be created from the sender,
    // this receiver does not need to be public accessible
    _num_listeners_receiver: tokio::sync::watch::Receiver<i32>,
    pub room_events: Sender<RoomEvent>,
}

impl BroadcastRoomMeta {
    pub fn new(name: String, uuid: Uuid, password: String, description: String) -> Self {
        let (num_listeners, _num_listeners_receiver) = watch::channel(0);
        let (room_events, _) = broadcast::channel(16);
        Self {
            name,
            uuid,
            admin_password: password,
            num_listeners,
            _num_listeners_receiver,
            room_events: room_events,
            description,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct RoomCreationReply {
    pub offer: String,
    pub password: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    AudioChannel,
    DataChannel(DataChannelKind),
}
