use std::sync::Arc;
use webrtc::{data_channel::RTCDataChannel, peer_connection::RTCPeerConnection};

#[derive(Clone)]
pub struct Connection {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Option<Arc<RTCDataChannel>>,
}
