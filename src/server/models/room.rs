use std::{fmt::Display, sync::Arc, time::Duration};

use async_graphql::{Enum, InputObject, Object, SimpleObject};
use shared::{
    connections::SteckerWebRTCConnection,
    models::{DataRoomInternalType, SteckerAudioChannel, SteckerData, SteckerDataChannelType},
};
use tokio::sync::broadcast::Sender;
use tracing::{error, info, instrument, trace, warn, Instrument, Span};
use uuid::Uuid;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use super::{audio_broadcast_room::AudioBroadcastRoom, data_broadcast_room::DataBroadcastRoom};

pub type ResponseOffer = String;

#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
// #[graphql(remote = "shared::models::RoomType")]
pub enum RoomType {
    Audio,
    DataRoomType,
}

pub enum DataRoomType {
    Chat,
    Float,
}

impl From<SteckerDataChannelType> for DataRoomType {
    fn from(value: SteckerDataChannelType) -> Self {
        match value {
            SteckerDataChannelType::Float => DataRoomType::Float,
            SteckerDataChannelType::String => DataRoomType::Chat,
        }
    }
}

impl Display for RoomType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            RoomType::DataRoomType(DataRoomType::Float) => write!(f, "FloatRoom"),
            RoomType::DataRoomType(DataRoomType::Chat) => write!(f, "ChatRoom"),
            RoomType::Audio => write!(f, "AudioRoom"),
        }
    }
}

// #[derive(Debug)]
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

#[derive(Debug, Clone)]
pub enum BroadcastRoomEvent {
    // contains new number of listeners in the room after listener joining
    ListenerJoined(i32),
    // contains new number of listeners in the room after listener leaving
    ListenerLeft(i32),
    // messages send from broadcaster to clients
    MetaBroadcastMessage(SteckerData),
    // messages send from client to broadcaster - currently ignored
    MetaReplyMessage(SteckerData),
}

#[derive(Debug)]
pub struct BroadcastRoomMeta {
    pub name: String,
    pub uuid: Uuid,
    pub admin_password: String,

    // pub brodacast_room_events: Sender<BroadcastRoomEvent>,
    pub num_listeners: tokio::sync::watch::Sender<i64>,
    // we need to keep the channel open, so we attach
    // a receiver to the "lifetime" of this struct.
    // as receivers can be created from the sender,
    // this receiver does not need to be public accessible
    // pub _num_listeners_receiver: tokio::sync::watch::Receiver<i64>,
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
