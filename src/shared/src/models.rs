use std::{collections::HashMap, fmt::Display, sync::{Arc, Mutex}};

use anyhow::anyhow;
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

pub trait SteckerSendable: Clone + Sync + 'static + Send + Display {
    // @todo can we return a reference here? would avoid much copying
    // and in a former implementation, where we attached it to the data
    // channel we could simply send out the reference
    fn to_stecker_data(&self) -> anyhow::Result<Bytes>;
    fn from_stecker_data(data: &DataChannelMessage) -> anyhow::Result<Self>;
}

#[derive(Clone)]
pub struct SteckerDataChannel<T: SteckerSendable> {
    // messages received from data channel are inbound,
    pub inbound: Sender<T>,
    // messages send to data channel are outbound
    pub outbound: Sender<T>,
    // triggers when connection was closed
    pub close: Sender<()>,
}


impl<T: SteckerSendable> SteckerDataChannel<T> {
    pub fn create_channels() -> Self {
        let capacity: usize = 1024;

        let (inbound, _) = broadcast::channel::<T>(capacity);
        let (outbound, _) = broadcast::channel::<T>(capacity);
        let (close, _) = broadcast::channel::<()>(1);

        SteckerDataChannel {
            inbound,
            outbound,
            close,
        }
    }
}

#[derive(Clone)]
pub enum SteckerData {
    F32(f32),
    String(String),
}

impl SteckerSendable for f32 {
    fn to_stecker_data(&self) -> anyhow::Result<Bytes> {
        let mut b = BytesMut::with_capacity(4);
        b.put_f32(*self);
        Ok(b.freeze())
    }

    fn from_stecker_data(data: &DataChannelMessage) -> anyhow::Result<Self> {
        let mut b = data.data.clone();
        Ok(Bytes::get_f32(&mut b))
    }
}

impl SteckerSendable for String {
    fn to_stecker_data(&self) -> anyhow::Result<Bytes> {
        Ok(self.clone().into())
    }

    fn from_stecker_data(data: &DataChannelMessage) -> anyhow::Result<Self> {
        Ok(String::from_utf8(data.data.to_vec())?)
    }
}

impl Display for SteckerData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            SteckerData::F32(v) => v.fmt(f),
            SteckerData::String(v) => v.fmt(f),
        }
    }
}

impl SteckerSendable for SteckerData {
    fn to_stecker_data(&self) -> anyhow::Result<Bytes> {
        match self {
            SteckerData::F32(float_sendable) => float_sendable.to_stecker_data(),
            SteckerData::String(string_sendable) => string_sendable.to_stecker_data(),
        }
    }

    fn from_stecker_data(_data: &DataChannelMessage) -> anyhow::Result<Self> {
        // what to do here?
        // i think we can't dispatch here - instead we fail?!
        // maybe this needs a better interface
        Err(anyhow!(
            "Cannot deserialize SteckerData without knowing the type"
        ))
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

pub struct DataChannelMap<T: SteckerSendable>(pub Mutex<HashMap<String, Arc<SteckerDataChannel<T>>>>);

impl<T: SteckerSendable> DataChannelMap<T> {
    pub fn insert(&self, channel_name: &str, stecker_channel: Arc<SteckerDataChannel<T>>) {
        self.0.lock().unwrap().insert(channel_name.to_string(), stecker_channel);
    }

    pub fn get(&self, channel_name: &str) -> Option<Arc<SteckerDataChannel<T>>> {
        self.0.lock().unwrap().get(channel_name).map(|a| a.clone())
    }
}
