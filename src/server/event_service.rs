use std::sync::Arc;

use rosc::{OscMessage, OscPacket};
use tokio::sync::RwLock;

use crate::models::room::BroadcastRoom;

#[derive(Clone)]
pub enum SteckerServerEvent {
    BroadcastRoomCreated(Arc<RwLock<BroadcastRoom>>),
    BroadcastRoomUpdated(Arc<RwLock<BroadcastRoom>>),
    BroadcastRoomDeleted(String),

    BroadcastRoomUserJoined(Arc<RwLock<BroadcastRoom>>),
    BroadcastRoomUserLeft(Arc<RwLock<BroadcastRoom>>),

    RoomDispatcherCreated(),
}

impl SteckerServerEvent {
    pub async fn into_osc_packet(self) -> Option<OscPacket> {
        match self {
            SteckerServerEvent::BroadcastRoomCreated(room) => {
                let room_name = rosc::OscType::String(room.read().await.meta().name.clone());
                Some(OscPacket::Message(OscMessage {
                    addr: "/room/created".to_string(),
                    args: vec![room_name],
                }))
            }
            SteckerServerEvent::BroadcastRoomUpdated(room) => {
                let room_name = rosc::OscType::String(room.read().await.meta().name.clone());
                Some(OscPacket::Message(OscMessage {
                    addr: "/room/update".to_string(),
                    args: vec![room_name],
                }))
            }
            SteckerServerEvent::BroadcastRoomDeleted(name) => {
                let room_name = rosc::OscType::String(name);
                Some(OscPacket::Message(OscMessage {
                    addr: "/room/deleted".to_string(),
                    args: vec![room_name],
                }))
            }
            SteckerServerEvent::RoomDispatcherCreated() => Some(OscPacket::Message(OscMessage {
                addr: "/dispatcher/created".to_string(),
                args: vec![],
            })),
            _ => None,
        }
    }
}
