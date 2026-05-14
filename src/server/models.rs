use std::{fmt::Display, sync::Arc, thread::JoinHandle, time::Duration};

use anyhow::anyhow;
use async_graphql::{Enum, Guard, InputObject, Object, SimpleObject};
use futures::stream::{self, StreamExt};
use rand::{
    distributions::{Alphanumeric, DistString},
    SeedableRng,
};
use rand::{rngs::StdRng, seq::SliceRandom};
use regex::Regex;
use shared::{
    connections::ConnectionEvent,
    models::{
        DataChannelEvent, RoomFloatData, RoomStringData, SteckerDataChanelTrait, SteckerDataChannel,
    },
};
use shared::{
    connections::SteckerWebRTCConnection,
    models::{SteckerAudioChannel, SteckerData},
};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::RwLock;
use tokio::{
    sync::{
        broadcast::{self, Sender},
        mpsc, oneshot, watch, Mutex,
    },
    task,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace, warn, Instrument, Span};
use uuid::Uuid;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::{
    dtls::conn,
    peer_connection::{self, peer_connection_state::RTCPeerConnectionState, RTCPeerConnection},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::event_service::RoomEvent;

// graphql objects

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum DispatcherType {
    Random,
    NextFreeAlphabetical,
    NextFreeRandom,
}

impl TryFrom<String> for DispatcherType {
    type Error = ();

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_ascii_lowercase().as_str() {
            "random" => Ok(DispatcherType::Random),
            "nextfreealpha" => Ok(DispatcherType::NextFreeAlphabetical),
            "nextfreerandom" => Ok(DispatcherType::NextFreeRandom),
            _ => Err(()),
        }
    }
}

impl DispatcherType {
    pub async fn choose_room(
        &self,
        rooms: Vec<Arc<RwLock<BroadcastRoom>>>,
    ) -> Option<BroadcastRoom> {
        return None;
        /*
        let mut empty_rooms: Vec<(String, Arc<RwLock<BroadcastRoom>>)> =
            stream::iter(rooms.clone())
                .then(|room| async move {
                    let (listeners, name) = {
                        let guard = room.read().await;
                        let listeners = *guard.meta().num_listeners.borrow();
                        let name = guard.meta().name.clone();
                        (listeners, name)
                    };
                    (listeners <= 0, name, room)
                })
                .filter(|(ok, _name, _room)| futures::future::ready(*ok))
                .map(|(_ok, name, room)| (name, room))
                .collect()
                .await;

        match self {
            DispatcherType::Random => {
                if let Some(room) = rooms.choose(&mut StdRng::from_entropy()) {
                    let room_lock = room.read().await;
                    return Some((&*room_lock).into());
                } else {
                    None
                }
            }
            DispatcherType::NextFreeAlphabetical => {
                empty_rooms.sort_by(|a, b| a.0.cmp(&b.0));
                match empty_rooms.first() {
                    Some((_, room)) => Some((&*room.read().await).into()),
                    None => None,
                }
            }
            DispatcherType::NextFreeRandom => {
                match empty_rooms.choose(&mut StdRng::from_entropy()) {
                    Some((_, room)) => Some((&*room.read().await).into()),
                    None => None,
                }
            }
        }
        */
    }
}

/// A dispatcher allows to select a room based on a given
/// regular expression and a given dispatcher_type.
#[derive(Clone)]
pub struct RoomDispatcher {
    /// name of the dispatcher - used for identification and must be unique
    pub name: String,
    /// only people with the password can modify the dispatcher until it is deleted
    /// or it is timed out.
    pub admin_password: String,
    /// each room which matches this regex will be considered a candidate
    pub rule: Regex,
    /// determines which dispatcher rule to apply on the filtered candidates
    pub dispatcher_type: DispatcherType,
    pub timeout_sender: tokio::sync::watch::Sender<Duration>,
    pub timeout_receiver: tokio::sync::watch::Receiver<Duration>,
    /// if set, the stream website will also create an audio back channel
    /// with the given prefix.
    pub return_room_prefix: Option<String>,
    /// if true there will also be a random string added postfix to the name
    /// of the back channel. this allows to have many people consuming an
    /// channel via a dispatcher but still receive the back-channel
    /// of each listener.
    pub add_random_postfix: bool,
}

// graphql conversion
#[Object]
impl RoomDispatcher {
    async fn name(&self) -> String {
        self.name.clone()
    }

    async fn rule(&self) -> String {
        self.rule.as_str().to_string()
    }

    async fn dispatcher_type(&self) -> DispatcherType {
        self.dispatcher_type
    }

    async fn return_room_prefix(&self) -> Option<String> {
        self.return_room_prefix.clone()
    }

    async fn append_random_postfix(&self) -> bool {
        self.add_random_postfix
    }
}

#[derive(InputObject, Clone)]
pub struct RoomDispatcherInput {
    pub name: String,
    pub admin_password: Option<String>,
    pub rule: String,
    pub room_type: RoomType,
    pub dispatcher_type: DispatcherType,
    pub timeout: i32,
    pub return_room_prefix: Option<String>,
    pub add_random_postfix: bool,
}

impl From<RoomDispatcherInput> for RoomDispatcher {
    fn from(value: RoomDispatcherInput) -> Self {
        let (timeout_sender, timeout_receiver) =
            tokio::sync::watch::channel(Duration::from_secs(value.timeout.try_into().unwrap()));
        RoomDispatcher {
            name: value.name,
            admin_password: if let Some(pw) = value.admin_password {
                pw
            } else {
                Alphanumeric.sample_string(&mut StdRng::from_entropy(), 8)
            },
            rule: Regex::new(&value.rule).unwrap(),
            dispatcher_type: value.dispatcher_type,
            timeout_sender,
            timeout_receiver,
            return_room_prefix: value.return_room_prefix,
            add_random_postfix: value.add_random_postfix,
        }
    }
}

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
    fn get_field(
        room: &BroadcastRoom,
    ) -> Arc<tokio::sync::RwLock<Option<Arc<SteckerDataChannel<T>>>>>;
}

impl ChannelAccess<RoomFloatData> for DataChannelKind {
    fn get_field(
        room: &BroadcastRoom,
    ) -> Arc<tokio::sync::RwLock<Option<Arc<SteckerDataChannel<RoomFloatData>>>>> {
        room.float_channel.clone()
    }
}

impl ChannelAccess<RoomStringData> for DataChannelKind {
    fn get_field(
        room: &BroadcastRoom,
    ) -> Arc<tokio::sync::RwLock<Option<Arc<SteckerDataChannel<RoomStringData>>>>> {
        room.chat_channel.clone()
    }
}

#[derive(Debug)]
pub struct BroadcastRoom {
    audio_channel: Arc<RwLock<Option<Arc<SteckerAudioChannel>>>>,
    float_channel: Arc<RwLock<Option<Arc<SteckerDataChannel<RoomFloatData>>>>>,
    chat_channel: Arc<RwLock<Option<Arc<SteckerDataChannel<RoomStringData>>>>>,
    /// if a stream gets taken over we must re-assign the audio_sequence_number b/c
    /// otherwhise the stream will think it has stalled, which will result in silence
    audio_sequence_offset: watch::Sender<u16>,

    // all metadata for a room is stored in a dedicated such that it can be cloned for schema access
    meta: BroadcastRoomMeta,

    pub free_room: broadcast::Sender<()>,
    active_channels: Arc<Mutex<u32>>,
    current_deletion_token: Arc<Mutex<CancellationToken>>,
    timeout: Duration,
}

type ResponseOffer = String;

impl BroadcastRoom {
    pub fn new(name: String, password: String, uuid: Uuid, description: String) -> Self {
        let (free_room, _) = broadcast::channel::<()>(1);
        let (audio_sequence_offset, _) = watch::channel(0);
        Self {
            audio_channel: Arc::new(RwLock::new(None)),
            float_channel: Arc::new(RwLock::new(None)),
            chat_channel: Arc::new(RwLock::new(None)),
            meta: BroadcastRoomMeta::new(name.clone(), uuid, password.clone(), description),
            current_deletion_token: Arc::new(Mutex::new(CancellationToken::new())),
            free_room,
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
        &self,
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
        &self,
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

        let _ = audio_channel.reset_sender.send(());

        let (connection_events, _) = broadcast::channel(256);
        let connection = Arc::new(
            SteckerWebRTCConnection::build_connection(connection_events)
                .in_current_span()
                .await?,
        );
        let offer = connection.respond_to_offer(&offer).await?;

        let audio_sequence_offset = self.audio_sequence_offset.clone();
        let room_name = self.meta.name.clone();
        let audio_channel_handle = self.audio_channel.clone();
        let room_deletion_token = self.current_deletion_token.clone();
        let trigger_free_room = self.free_room.clone();
        let active_channels = self.active_channels.clone();
        let room_timeout = self.timeout;

        tokio::spawn(
            async move {
                room_deletion_token.lock().await.cancel();
                *active_channels.lock().await += 1;

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
                    let local_track = Arc::new(TrackLocalStaticRTP::new(
                        audio_track.codec().capability,
                        "audio".to_string(),
                        "stecker".to_string(),
                    ));
                    let _ = audio_channel
                        .audio_channel_tx
                        .send(Some(local_track.clone()));
                    let _ = room_events.send(RoomEvent::BroadcastRoomUpdated(room_name));

                    let mut reset_rx = audio_channel.reset_sender.subscribe();
                    let offset = *audio_sequence_offset.borrow();
                    loop {
                        tokio::select! {
                            Ok((mut rtp, _)) = audio_track.read_rtp() => {
                                let mut seq_number = rtp.header.sequence_number;
                                seq_number = seq_number.wrapping_add(offset);
                                let _ = audio_sequence_offset.send(seq_number);
                                rtp.header.sequence_number = seq_number;
                                let _ = local_track.write_rtp(&rtp).await;
                            },
                            _ = connection.wait_for_disconnect() => break,
                            _ = reset_rx.recv() => {
                                info!("Sender replaced again");
                                let _ = connection.close().await;
                                *active_channels.lock().await -= 1;
                                return;
                            }
                            else => break,
                        }
                    }
                }

                let _ = connection.close().await;
                trace!("Release audio channel");
                *audio_channel_handle.write().await = None;

                Self::check_deletion(
                    active_channels,
                    room_deletion_token,
                    room_timeout,
                    trigger_free_room,
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
        trigger_free_room: Sender<()>,
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
                let _ = trigger_free_room.send(());
            }
        }
    }

    #[instrument(skip_all)]
    pub async fn create_data_channel<T>(
        &self,
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
        let data_channel_handle = DataChannelKind::get_field(&self);
        *data_channel_handle.write().await = Some(data_channel);

        // an async callback to see if a float channel has been set and also release it afterwards
        // let data_channel_handle = data_channel_handle;
        let room_deletion_token = self.current_deletion_token.clone();
        let trigger_free_room = self.free_room.clone();
        let active_channels = self.active_channels.clone();
        let room_timeout = self.timeout.clone();
        tokio::spawn(async move {
            // alternative: bump the duration on connection success
            let mut active_timeout = true;
            // @todo this can create a race condition b/c maybe in the meantime the room already got deleted?
            // but if we trigger the cancellation earlier this can also lead to a dangling room while if e.g.
            // build_connection fails.
            room_deletion_token.lock().await.cancel();
            *active_channels.lock().await += 1;
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
            *data_channel_handle.write().await = None;

            let _ = Self::check_deletion(active_channels, room_deletion_token, room_timeout, trigger_free_room).await;
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
        &self,
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

        // an async callback to see if a float channel has been set and also release it afterwards
        // let data_channel_handle = data_channel_handle;
        let room_deletion_token = self.current_deletion_token.clone();
        let trigger_free_room = self.free_room.clone();
        let active_channels = self.active_channels.clone();
        let room_timeout = self.timeout.clone();
        let audio_channel_handle = self.audio_channel.clone();
        let audio_sequence_offset = self.audio_sequence_offset.clone();
        let room_name = self.meta.name.clone();

        tokio::spawn(
            async move {
                room_deletion_token.lock().await.cancel();
                *active_channels.lock().await += 1;
                let mut reset_rx = audio_channel_clone.reset_sender.subscribe();
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
                    let _ = room_events.send(RoomEvent::BroadcastRoomStreaming(room_name));

                    let offset = *audio_sequence_offset.borrow();
                    loop {
                        tokio::select! {
                            Ok((mut rtp, _)) = audio_track.read_rtp() => {
                                let mut seq_number = rtp.header.sequence_number;
                                seq_number = seq_number.wrapping_add(offset);
                                let _ = audio_sequence_offset.send(seq_number);
                                rtp.header.sequence_number = seq_number;
                                let _ = local_track.write_rtp(&rtp).await;
                            },
                            _ = connection.wait_for_disconnect() => break,
                            _ = reset_rx.recv() => {
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
                    *active_channels.lock().await -= 1;
                } else {
                    trace!("Release audio channel");
                    *audio_channel_handle.write().await = None;

                    Self::check_deletion(
                        active_channels,
                        room_deletion_token,
                        room_timeout,
                        trigger_free_room,
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

// impl Into<RoomType> for DataRoomInternalType {
//     fn into(self) -> RoomType {
//         match self {
//             DataRoomInternalType::Float => RoomType::Float,
//             DataRoomInternalType::Chat => RoomType::Chat,
//             // @todo meta rooms do not exist exposed to the graphql api
//             DataRoomInternalType::Meta => !unimplemented!(),
//         }
//     }
// }

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
