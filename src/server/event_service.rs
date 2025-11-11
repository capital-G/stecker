use rosc::{OscMessage, OscPacket};

#[derive(Debug, Clone)]
pub enum RoomEvent {
    BroadcastRoomCreated(String),
    BroadcastRoomUpdated(String),
    BroadcastRoomUserJoined(String, i32),
    BroadcastRoomUserLeft(String, i32),
    BroadcastRoomDeleted(String),

    RoomDispatcherCreated(String),
    RoomDispatcherDeleted(String),
    RoomDispatcherReset(),
}

impl RoomEvent {
    pub async fn into_osc_packet(self) -> OscPacket {
        match self {
            RoomEvent::BroadcastRoomCreated(room_name) => OscPacket::Message(OscMessage {
                addr: "/createdRoom".to_string(),
                args: vec![rosc::OscType::String(room_name)],
            }),
            RoomEvent::BroadcastRoomUpdated(room_name) => {
                let updated = rosc::OscType::String("updated".to_string());
                let room = rosc::OscType::String(room_name);
                OscPacket::Message(OscMessage {
                    addr: "/updatedRoom".to_string(),
                    args: vec![room.clone()],
                });
                OscPacket::Message(OscMessage {
                    addr: "/room/{room_name}".to_string(),

                    args: vec![updated, room],
                })
            }
            RoomEvent::BroadcastRoomDeleted(room_name) => {
                let deleted = rosc::OscType::String("deleted".to_string());
                let room = rosc::OscType::String(room_name);
                OscPacket::Message(OscMessage {
                    addr: "/deletedRoom".to_string(),
                    args: vec![room.clone()],
                });
                OscPacket::Message(OscMessage {
                    addr: "/room".to_string(),
                    args: vec![deleted, room],
                })
            }
            RoomEvent::BroadcastRoomUserJoined(room_name, new_num_listeners) => {
                let room_joined = rosc::OscType::String("joined".to_string());
                OscPacket::Message(OscMessage {
                    addr: "/room".to_string(),
                    args: vec![
                        rosc::OscType::String(room_name),
                        room_joined,
                        rosc::OscType::Int(new_num_listeners),
                    ],
                })
            }
            RoomEvent::BroadcastRoomUserLeft(room_name, new_num_listeners) => {
                let room_left = rosc::OscType::String("left".to_string());

                OscPacket::Message(OscMessage {
                    addr: "/room".to_string(),
                    args: vec![
                        rosc::OscType::String(room_name),
                        room_left,
                        rosc::OscType::Int(new_num_listeners),
                    ],
                })
            }

            RoomEvent::RoomDispatcherCreated(name) => OscPacket::Message(OscMessage {
                addr: "/createdDispatcher".to_string(),
                args: vec![rosc::OscType::String(name)],
            }),
            RoomEvent::RoomDispatcherDeleted(name) => OscPacket::Message(OscMessage {
                addr: "/deletedDispatcher".to_string(),
                args: vec![rosc::OscType::String(name)],
            }),
            RoomEvent::RoomDispatcherReset() => OscPacket::Message(OscMessage {
                addr: "/resetDispatcher".to_string(),
                args: vec![],
            }),
        }
    }
}
