use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex, RwLock},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::broadcast::{self, Sender};
use webrtc::{
    data_channel::data_channel_message::DataChannelMessage,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

// @todo use cargo.toml version
pub static API_VERSION: &'static str = "0.1.0";

/// the possible kinds of data rooms used
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DataRoomType {
    Float,
    Chat,
    Meta,
}

/// the possible kinds of data channels
/// that are exposed for using via the server or the client.
#[derive(Copy, Clone)]
pub enum DataRoomData {
    Float,
    Chat,
}

impl From<SteckerDataChannelType> for DataRoomInternalType {
    fn from(value: SteckerDataChannelType) -> Self {
        match value {
            SteckerDataChannelType::Float => DataRoomInternalType::Float,
            SteckerDataChannelType::String => DataRoomInternalType::Chat,
        }
    }
}

pub enum SteckerAPIRoomType {
    Audio,
    Data(DataRoomPublicType),
}

impl From<DataRoomInternalType> for SteckerDataChannelType {
    fn from(value: DataRoomInternalType) -> Self {
        match value {
            DataRoomInternalType::Float => SteckerDataChannelType::Float,
            DataRoomInternalType::Chat => SteckerDataChannelType::String,
            DataRoomInternalType::Meta => SteckerDataChannelType::String,
        }
    }
}

impl Into<DataRoomInternalType> for DataRoomPublicType {
    fn into(self) -> DataRoomInternalType {
        match self {
            DataRoomPublicType::Float => DataRoomInternalType::Float,
            DataRoomPublicType::Chat => DataRoomInternalType::Chat,
        }
    }
}

impl Display for DataRoomInternalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            DataRoomInternalType::Float => write!(f, "FloatRoom"),
            DataRoomInternalType::Chat => write!(f, "ChatRoom"),
            DataRoomInternalType::Meta => write!(f, "Meta"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ConnectionState {
    WaitingForConnection,
    Connected,
    Timeout,
}

#[derive(Clone, Debug)]
pub enum SteckerDataChannelEvent {
    /// messages received from data channel are inbound
    InboundMessage(SteckerData),
    /// messages send to data channel are outbound
    OutboundMessage(SteckerData),
    ConnectionClosed,
}

#[derive(Clone)]
/// Acts as an abstraction
pub struct SteckerDataChannel {
    /// label of the data channel
    // @todo is this obsolete b/c we have channel type?
    pub label: String,
    pub events: Sender<SteckerDataChannelEvent>,
    // necessary for async matching via listening
    // on data channels.
    pub channel_type: SteckerDataChannelType,
}

impl SteckerDataChannel {
    pub fn create_channels(channel_type: SteckerDataChannelType) -> Self {
        let (events, _) = broadcast::channel::<SteckerDataChannelEvent>(64);
        SteckerDataChannel {
            label: ChannelName::from(&DataRoomInternalType::from(channel_type)),
            events,
            channel_type,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SteckerData {
    F32(f32),
    String(String),
}

impl SteckerData {
    pub fn encode(&self) -> anyhow::Result<Bytes> {
        match self {
            SteckerData::F32(data) => {
                let mut b = BytesMut::with_capacity(4);
                b.put_f32(*data);
                Ok(b.freeze())
            }
            SteckerData::String(data) => Ok(data.clone().into()),
        }
    }

    pub fn decode_float(data: DataChannelMessage) -> anyhow::Result<SteckerData> {
        let mut b = data.data.clone();
        // @todo what happens if we later match against the non existing "String" here?!
        Ok(Self::F32(Bytes::get_f32(&mut b)))
    }

    pub fn to_float(data: DataChannelMessage) -> f32 {
        let mut b = data.data.clone();
        Bytes::get_f32(&mut b)
    }

    pub fn decode_string(data: DataChannelMessage) -> anyhow::Result<SteckerData> {
        Ok(Self::String(String::from_utf8(data.data.to_vec())?))
    }

    pub fn to_string(data: DataChannelMessage) -> String {
        unsafe { String::from_utf8_unchecked(data.data.to_vec()) }
    }
}

impl Display for SteckerData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            SteckerData::F32(value) => write!(f, "F32({})", value),
            SteckerData::String(value) => write!(f, "String({})", value),
        }
    }
}

pub type ChannelName = String;

impl From<&DataRoomInternalType> for ChannelName {
    fn from(value: &DataRoomInternalType) -> Self {
        match value {
            DataRoomInternalType::Float => "float".to_string(),
            DataRoomInternalType::Chat => "chat".to_string(),
            DataRoomInternalType::Meta => "meta".to_string(),
        }
    }
}

pub struct DataChannelMap(pub RwLock<HashMap<String, Arc<SteckerDataChannel>>>);

impl DataChannelMap {
    pub fn insert(&self, channel_name: &str, stecker_channel: Arc<SteckerDataChannel>) {
        self.0
            .write()
            .unwrap()
            .insert(channel_name.to_string(), stecker_channel);
    }

    pub fn get(&self, channel_name: &str) -> Option<Arc<SteckerDataChannel>> {
        self.0.read().unwrap().get(channel_name).map(|a| a.clone())
    }
}

pub enum SteckerAudioChannelEvent {}

#[derive(Clone, Debug)]
pub struct SteckerAudioChannel {
    // channel which we use to store the current audio channel from which we receive
    pub audio_channel_rx: tokio::sync::watch::Receiver<Option<Arc<TrackLocalStaticRTP>>>,
    // channel which we use to push an audio channel to our consumers
    audio_channel_tx: tokio::sync::watch::Sender<Option<Arc<TrackLocalStaticRTP>>>,
    // drops the current source WebRTC connection so it can be replaced by a new one
    // pub reset_sender: Sender<()>,
    // if we want to replace a running sender, we also need to continue the sequence_number
    // of the RTP packages
    pub sequence_number_sender: tokio::sync::watch::Sender<u16>,
    // we also add the receiver b/c if we subscribe later we will not get the values before subscription
    pub sequence_number_receiver: tokio::sync::watch::Receiver<u16>,
}

impl SteckerAudioChannel {
    pub fn new() -> Self {
        // let (close, _) = broadcast::channel::<()>(1);
        let (audio_channel_tx, audio_channel_rx) = tokio::sync::watch::channel(None);
        // let (reset_sender, _) = broadcast::channel::<()>(1);
        let (sequence_number_sender, sequence_number_receiver) =
            tokio::sync::watch::channel::<u16>(0);
        SteckerAudioChannel {
            audio_channel_tx,
            audio_channel_rx,
            // close,
            // reset_sender,
            sequence_number_sender,
            sequence_number_receiver,
        }
    }

    pub fn listen_for_audio_channel() {}

    pub fn change_sender(&self, track: Arc<TrackLocalStaticRTP>) {
        let _ = self.audio_channel_tx.send(Some(track));
    }
}
