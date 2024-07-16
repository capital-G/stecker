use async_graphql::Enum;
use tokio::sync::broadcast::Sender;
use uuid::Uuid;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum RoomType {
    Float,
    Chat,
    Audio,
}

pub struct BroadcastRoom {
    pub name: String,
    // Reply to server (messages not broadcasted)
    // potentially not interesting to subscribe to this
    pub reply: Sender<f32>,
    // Subscribe to this to receive messages from room
    // potentially not useful to send to this (unless you also become a broadcaster)
    pub broadcast: Sender<f32>,
    pub uuid: Uuid,
    pub room_type: RoomType,
}
