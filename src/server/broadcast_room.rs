use tokio::sync::broadcast::Sender;
use uuid::Uuid;

pub struct BroadcastRoom {
    pub name: String,
    // Reply to server (messages not broadcasted)
    // potentially not interesting to subscribe to this
    pub reply: Sender<String>,
    // Subscribe to this to receive messages from room
    // potentially not useful to send to this (unless you also become a broadcaster)
    pub broadcast: Sender<String>,
    pub uuid: Uuid,
}
