use clap::ValueEnum;
use shared::models::{DataRoomType, PublicRoomType};

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

impl From<ClientRoomType> for DataRoomType {
    fn from(value: ClientRoomType) -> Self {
        match value {
            ClientRoomType::Chat => DataRoomType::Chat,
            ClientRoomType::Float => DataRoomType::Float,
        }
    }
}
