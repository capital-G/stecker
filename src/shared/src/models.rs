use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::broadcast::{self, Sender};
use webrtc::data_channel::data_channel_message::DataChannelMessage;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum RoomType {
    Float,
    Chat,
    Meta,
}

// users can not create a meta channel
#[derive(Copy, Clone)]
pub enum PublicRoomType {
    Float,
    Chat,
}

#[derive(Clone, Copy)]
pub enum SteckerDataChannelType {
    Float,
    String,
}

impl From<RoomType> for SteckerDataChannelType {
    fn from(value: RoomType) -> Self {
        match value {
            RoomType::Float => SteckerDataChannelType::Float,
            RoomType::Chat => SteckerDataChannelType::String,
            RoomType::Meta => SteckerDataChannelType::String,
        }
    }
}

impl Into<RoomType> for PublicRoomType {
    fn into(self) -> RoomType {
        match self {
            PublicRoomType::Float => RoomType::Float,
            PublicRoomType::Chat => RoomType::Chat,
        }
    }
}

impl Display for RoomType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            RoomType::Float => write!(f, "FloatRoom"),
            RoomType::Chat => write!(f, "ChatRoom"),
            RoomType::Meta => write!(f, "Meta"),
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

impl From<&RoomType> for ChannelName {
    fn from(value: &RoomType) -> Self {
        match value {
            RoomType::Float => "float".to_string(),
            RoomType::Chat => "chat".to_string(),
            RoomType::Meta => "meta".to_string(),
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
