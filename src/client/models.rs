use clap::ValueEnum;
use shared::models::{PublicRoomType, RoomType};

#[derive(ValueEnum, Clone)]
pub enum ClientRoomType {
    Chat,
    Float,
}

impl From<ClientRoomType> for PublicRoomType {
    fn from(value: ClientRoomType) -> Self {
        match value {
            ClientRoomType::Chat => PublicRoomType::Chat,
            ClientRoomType::Float => PublicRoomType::Float,
        }
    }
}

impl From<ClientRoomType> for RoomType {
    fn from(value: ClientRoomType) -> Self {
        match value {
            ClientRoomType::Chat => RoomType::Chat,
            ClientRoomType::Float => RoomType::Float,
        }
    }
}
