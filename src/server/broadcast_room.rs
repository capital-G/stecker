use std::sync::{Arc, Mutex};

use shared::connections::SteckerWebRTCConnection;

pub struct BroadcastRoom {
    pub name: String,
    pub source_connection: Arc<SteckerWebRTCConnection>,
    // pub source_connection: Option<Arc<Mutex<SteckerWebRTCConnection>>>,
    pub target_connections: Mutex<Vec<Arc<Mutex<SteckerWebRTCConnection>>>>,
    // pub rx: Receiver<i32>,
}
