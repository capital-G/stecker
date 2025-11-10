use std::sync::Arc;

use rosc::{OscMessage, OscPacket};
use tokio::sync::RwLock;

use crate::models::BroadcastRoom;

#[derive(Debug, Clone)]
pub enum RoomEvent {
    BroadcastRoomCreated(Arc<RwLock<BroadcastRoom>>),
    BroadcastRoomUpdated(Arc<RwLock<BroadcastRoom>>),
    BroadcastRoomDeleted(String),

    RoomDispatcherCreated(String),
    RoomDispatcherReset(),
}

impl RoomEvent {
    pub async fn into_osc_packet(self) -> OscPacket {
        match self {
            RoomEvent::BroadcastRoomCreated(room) => {
                let room_name = rosc::OscType::String(room.read().await.meta().name.clone());
                OscPacket::Message(OscMessage {
                    addr: "/room/created".to_string(),
                    args: vec![room_name],
                })
            }
            RoomEvent::BroadcastRoomUpdated(room) => {
                let room_name = rosc::OscType::String(room.read().await.meta().name.clone());
                OscPacket::Message(OscMessage {
                    addr: "/room/update".to_string(),
                    args: vec![room_name],
                })
            }
            RoomEvent::BroadcastRoomDeleted(name) => {
                let room_name = rosc::OscType::String(name);
                OscPacket::Message(OscMessage {
                    addr: "/room/deleted".to_string(),
                    args: vec![room_name],
                })
            }
            RoomEvent::RoomDispatcherCreated(name) => OscPacket::Message(OscMessage {
                addr: "/dispatcher/created".to_string(),
                args: vec![rosc::OscType::String(name)],
            }),
            RoomEvent::RoomDispatcherReset() => OscPacket::Message(OscMessage {
                addr: "/dispatcher/reset".to_string(),
                args: vec![],
            }),
        }
    }
}
