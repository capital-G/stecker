use std::sync::{Arc, Mutex};

use shared::connections::SteckerWebRTCConnection;
use tokio::sync::broadcast::Sender;

pub struct BroadcastRoom {
    pub name: String,
    // Reply to server (messages not broadcasted)
    // potentially not interesting to subscribe to this
    pub reply: Sender<String>,
    // Subscribe to this to receive messages from room
    // potentially not useful to send to this (unless you also become a broadcaster)
    pub broadcast: Sender<String>,
    // potentially obsolete
    pub source_connection: Arc<SteckerWebRTCConnection>,
    pub target_connections: Mutex<Vec<Arc<Mutex<SteckerWebRTCConnection>>>>,
}
