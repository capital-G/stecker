use clap::ValueEnum;
use shared::models::{DataRoomInternalType, DataRoomPublicType, SteckerAPIRoomType};

#[derive(ValueEnum, Clone)]
pub enum ClientRoomType {
    Chat,
    Float,
}

impl From<ClientRoomType> for DataRoomPublicType {
    fn from(value: ClientRoomType) -> Self {
        match value {
            ClientRoomType::Chat => DataRoomPublicType::Chat,
            ClientRoomType::Float => DataRoomPublicType::Float,
        }
    }
}

impl From<ClientRoomType> for DataRoomInternalType {
    fn from(value: ClientRoomType) -> Self {
        match value {
            ClientRoomType::Chat => DataRoomInternalType::Chat,
            ClientRoomType::Float => DataRoomInternalType::Float,
        }
    }
}

impl From<ClientRoomType> for SteckerAPIRoomType {
    fn from(value: ClientRoomType) -> Self {
        match value {
            ClientRoomType::Chat => SteckerAPIRoomType::Data(DataRoomPublicType::Chat),
            ClientRoomType::Float => SteckerAPIRoomType::Data(DataRoomPublicType::Float),
        }
    }
}
