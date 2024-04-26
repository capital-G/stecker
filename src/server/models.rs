use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;
use webrtc::{data_channel::RTCDataChannel, peer_connection::RTCPeerConnection, util::Conn};

#[derive(Clone)]
pub struct User {
    pub uuid: String,
    pub name: String,
    pub connection: Connection,
}


#[derive(Clone)]
pub struct Connection {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Option<Arc<RTCDataChannel>>
}

// #[derive(Clone)]
pub struct BroadcastRoom {
    pub name: String,
    pub source_connection: Option<Connection>,
    pub target_connections: Mutex<Vec<Connection>>,
    // pub listenCallback: dyn FnMut(),
    // pub peer_connection: Arc<RTCPeerConnection>,
}

struct Message {
    text: String,
}

struct WebRTCConnection {

}

struct Room {
    uuid: String,
    name: String,
    connections: Vec<WebRTCConnection>,
}

trait RoomManager {
    async fn create_room(&mut self, name: String) -> String;
    async fn join_room(&mut self, room_name: String, connection: WebRTCConnection);
    async fn send_message(&mut self, room_uuid: String, message: Message); 
}

struct RoomManagerImpl {
    rooms: HashMap<String, Room>
    
}

impl RoomManagerImpl {
    fn new() -> Self {
        RoomManagerImpl { rooms: HashMap::new() }
    }
}

impl RoomManager for RoomManagerImpl {
    async fn create_room(&mut self, room_name: String) -> String {
        let uuid = Arc::new(Uuid::new_v4());
        let room = Room{
            uuid: uuid.clone().to_string(),
            name: room_name,
            connections: Vec::new(),
        };
        self.rooms.insert(uuid.clone().to_string(), room);
        uuid.to_owned().to_string()
    }

    async fn join_room(&mut self, room_name: String, connection: WebRTCConnection) {
        if let Some(room) = self.rooms.get_mut(&room_name) {
            room.connections.push(connection);
        }
    }

    async fn send_message(&mut self, room_uuid: String, message: Message) {
        if let Some(room) = self.rooms.get(&room_uuid) {
            // @todo
        }
    }
}
