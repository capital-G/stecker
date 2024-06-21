use std::sync::Arc;
use tokio::sync::{mpsc::Receiver, Mutex};
use webrtc::{data_channel::RTCDataChannel, peer_connection::RTCPeerConnection};


#[derive(Clone)]
pub struct Connection {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Option<Arc<RTCDataChannel>>,
}

pub struct BroadcastRoom {
    pub name: String,
    pub source_connection: Option<Arc<Mutex<Connection>>>,
    pub target_connections: Mutex<Vec<Arc<Mutex<Connection>>>>,
    pub rx: Receiver<i32>,
}
