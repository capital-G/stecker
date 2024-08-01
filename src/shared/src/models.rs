use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::broadcast::{self, Sender};
use webrtc::{
    data_channel::data_channel_message::DataChannelMessage,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

/// the possible kinds of data rooms
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum DataRoomType {
    Float,
    Chat,
    Meta,
}

/// the possible kinds of data channels
/// that are usable for the public
/// via the server.
#[derive(Copy, Clone)]
pub enum PublicRoomType {
    Float,
    Chat,
}

/// the "raw" possible kinds of data channels
/// which will be used internally
#[derive(Clone, Copy)]
pub(crate) enum SteckerDataChannelType {
    Float,
    String,
}

impl From<DataRoomType> for SteckerDataChannelType {
    fn from(value: DataRoomType) -> Self {
        match value {
            DataRoomType::Float => SteckerDataChannelType::Float,
            DataRoomType::Chat => SteckerDataChannelType::String,
            DataRoomType::Meta => SteckerDataChannelType::String,
        }
    }
}

impl Into<DataRoomType> for PublicRoomType {
    fn into(self) -> DataRoomType {
        match self {
            PublicRoomType::Float => DataRoomType::Float,
            PublicRoomType::Chat => DataRoomType::Chat,
        }
    }
}

impl Display for DataRoomType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            DataRoomType::Float => write!(f, "FloatRoom"),
            DataRoomType::Chat => write!(f, "ChatRoom"),
            DataRoomType::Meta => write!(f, "Meta"),
        }
    }
}

#[derive(Clone)]
pub struct SteckerDataChannel {
    /// messages received from data channel are inbound,
    pub inbound: Sender<SteckerData>,
    /// messages send to data channel are outbound
    pub outbound: Sender<SteckerData>,
    /// triggers when connection was closed
    pub close: Sender<()>,
    // necessary for async matching via listening
    // on data channels.
    pub channel_type: SteckerDataChannelType,
}

impl SteckerDataChannel {
    pub fn create_channels(channel_type: SteckerDataChannelType) -> Self {
        let capacity: usize = 1024;

        let (inbound, _) = broadcast::channel::<SteckerData>(capacity);
        let (outbound, _) = broadcast::channel::<SteckerData>(capacity);
        let (close, _) = broadcast::channel::<()>(1);

        SteckerDataChannel {
            inbound,
            outbound,
            close,
            channel_type,
        }
    }
}

#[derive(Clone)]
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

    pub fn decode_string(data: DataChannelMessage) -> anyhow::Result<SteckerData> {
        Ok(Self::String(String::from_utf8(data.data.to_vec())?))
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

impl From<&DataRoomType> for ChannelName {
    fn from(value: &DataRoomType) -> Self {
        match value {
            DataRoomType::Float => "float".to_string(),
            DataRoomType::Chat => "chat".to_string(),
            DataRoomType::Meta => "meta".to_string(),
        }
    }
}

pub struct DataChannelMap(pub Mutex<HashMap<String, Arc<SteckerDataChannel>>>);

impl DataChannelMap {
    pub fn insert(&self, channel_name: &str, stecker_channel: Arc<SteckerDataChannel>) {
        self.0
            .lock()
            .unwrap()
            .insert(channel_name.to_string(), stecker_channel);
    }

    pub fn get(&self, channel_name: &str) -> Option<Arc<SteckerDataChannel>> {
        self.0.lock().unwrap().get(channel_name).map(|a| a.clone())
    }
}

#[derive(Clone)]
pub struct SteckerAudioChannel {
    // pub audio_channel_tx: Sender<Arc<TrackLocalStaticRTP>>,
    pub audio_channel_rx: tokio::sync::watch::Receiver<Option<Arc<TrackLocalStaticRTP>>>,
    pub audio_channel_tx: tokio::sync::watch::Sender<Option<Arc<TrackLocalStaticRTP>>>,
    pub close: Sender<()>,
}

impl SteckerAudioChannel {
    pub fn create_channels() -> Self {
        let (close, _) = broadcast::channel::<()>(1);
        let (audio_channel_tx, audio_channel_rx) = tokio::sync::watch::channel(None);
        // let (audio_channel_tx, _) = broadcast::channel::<Arc<TrackLocalStaticRTP>>(1);
        SteckerAudioChannel {
            audio_channel_tx,
            audio_channel_rx,
            close,
        }
    }
}
