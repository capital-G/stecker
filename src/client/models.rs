use clap::ValueEnum;
use shared::models::PublicRoomType;

#[derive(ValueEnum, Clone)]
pub enum ClientRoomType {
    Chat,
    Float,
}

impl Into<PublicRoomType> for ClientRoomType {
    fn into(self) -> PublicRoomType {
        match self {
            ClientRoomType::Chat => PublicRoomType::Chat,
            ClientRoomType::Float => PublicRoomType::Float,
        }
    }
}
