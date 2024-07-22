use std::fmt::Display;

use anyhow::anyhow;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::broadcast::Sender;
use webrtc::data_channel::data_channel_message::DataChannelMessage;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum RoomType {
    Float,
    Chat,
    Meta,
}

// users can not create a meta channel
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

impl RoomType {
    pub fn get_default_label(&self) -> &'static str {
        match &self {
            RoomType::Float => "float",
            RoomType::Chat => "chat",
            RoomType::Meta => "meta",
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

pub struct SteckerDataChannel<T: SteckerSendable> {
    pub inbound: Sender<T>,
    pub outbound: Sender<T>,
    pub close_trigger: Sender<()>,
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
