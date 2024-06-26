use std::sync::Arc;
use tokio::sync::{mpsc::Receiver, Mutex};
use webrtc::{data_channel::RTCDataChannel, peer_connection::RTCPeerConnection};

use crate::connections::SteckerWebRTCConnection;

#[derive(Clone)]
pub struct Connection {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub data_channel: Option<Arc<RTCDataChannel>>,
}
