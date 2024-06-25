use crate::models::{Connection};
use crate::utils::{decode_b64, encode_offer};

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use std::sync::Mutex as SyncMutex;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use std::sync::Arc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;

pub struct ConnectionWithOffer {
    pub connection: Arc<Mutex<Connection>>,
    pub offer: String,
}

impl Connection {
    pub async fn respond_to_offer(
        offer: String,
        tx: Arc<Sender<String>>,
    ) -> anyhow::Result<ConnectionWithOffer> {
        let desc_data = decode_b64(&offer)?;
        let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

        let mut m = MediaEngine::default();
        m.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut m)?;

        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        let connection = Arc::new(Mutex::new(Connection {
            peer_connection: peer_connection.clone(),
            data_channel: None,
        }));

        let connection_clone = connection.clone();

        peer_connection.on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
            let tx2 = tx.clone();

            d.on_close(Box::new(|| {
                println!("Data channel closed");
                Box::pin(async {})
            }));

            d.on_open(Box::new(move || {
                println!("Opened channel");
                Box::pin(async move {
                })
            }));

            d.on_message(Box::new(move |message: DataChannelMessage| {
                let msg_str = String::from_utf8(message.data.to_vec()).unwrap();
                let msg2 = msg_str.clone();
                
                let tx3 = tx2.clone();

                Box::pin(async move {
                    match tx3.send(msg2).await {
                        Ok(_) => (),
                        Err(x) => println!("Could not forward message: {x}"),
                    }
                })
            }));

            let c2 = connection.clone();

            Box::pin(async move {
                c2.lock().await.data_channel = Some(d.clone());
            })
        }));

        peer_connection.set_remote_description(offer).await?;
        let answer = peer_connection.create_answer(None).await?;

        // Create channel that is blocked until ICE Gathering is complete
        let mut gather_complete = peer_connection.gathering_complete_promise().await;

        // Sets the LocalDescription, and starts our UDP listeners
        peer_connection.set_local_description(answer).await?;

        // Block until ICE Gathering is complete, disabling trickle ICE
        // we do this because we only can exchange one signaling message
        // in a production application you should exchange ICE Candidates via OnICECandidate
        let _ = gather_complete.recv().await;

        // Output the answer in base64 so we can paste it in browser
        let offer = if let Some(local_desc) = peer_connection.local_description().await {
            let b64 = encode_offer(local_desc)?;
            Ok(b64)
        } else {
            Err(anyhow::anyhow!("Error while creating RTC offer"))
        };

        Ok(ConnectionWithOffer {
            connection: connection_clone,
            offer: offer?,
        })
    }
}

// impl BroadcastRoom {
//     pub async fn create_new_room(name: String) -> anyhow::Result<Self> {
//         let (_, done_rx) = tokio::sync::mpsc::channel::<i32>(1);

//         let broadcast_room = BroadcastRoom {
//             name,
//             source_connection: None,
//             target_connections: Mutex::new(vec![]),
//             rx: done_rx,
//         };

//         Ok(broadcast_room)
//     }

//     pub async fn join_room(&mut self, connection: Arc<Mutex<Connection>>) {
//         self.target_connections.get_mut().push(connection);
//     }
// }


pub struct SteckerWebRTCConnection {
    pub peer_connection: RTCPeerConnection,
    // we use a tokio mutex b/c we need to access it
    // from different threads
    data_channel: Mutex<Option<Arc<RTCDataChannel>>>,
}


impl SteckerWebRTCConnection {
    pub async fn build_connection() -> anyhow::Result<Self> {
        let mut m = MediaEngine::default();
        m.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut m)?;

        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        Ok(Self {
            peer_connection: api.new_peer_connection(config).await?,
            data_channel: Mutex::new(None),
        })
    }

    pub async fn respond_to_offer(&self, offer: String) -> anyhow::Result<String> {
        let desc_data = decode_b64(&offer)?;
        let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

        self.peer_connection.set_remote_description(offer).await?;
        let answer = self.peer_connection.create_answer(None).await?;

        // Create channel that is blocked until ICE Gathering is complete
        let mut gather_complete = self.peer_connection.gathering_complete_promise().await;

        // Sets the LocalDescription, and starts our UDP listeners
        self.peer_connection.set_local_description(answer).await?;

        // Block until ICE Gathering is complete, disabling trickle ICE
        // we do this because we only can exchange one signaling message
        // in a production application you should exchange ICE Candidates via OnICECandidate
        let _ = gather_complete.recv().await;

        let offer = if let Some(local_desc) = self.peer_connection.local_description().await {
            let b64 = encode_offer(local_desc)?;
            Ok(b64)
        } else {
            Err(anyhow::anyhow!("Error while creating RTC offer"))
        };

        offer
    }

    pub async fn listen_for_data_channel(self: Arc<Self>) -> Receiver<String> {
        let self2 = self.clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(1);

        self.peer_connection.on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
            let tx2 = tx.clone();

            d.on_close(Box::new(|| {
                println!("Data channel closed");
                Box::pin(async {})
            }));

            d.on_open(Box::new(move || {
                println!("Opened channel");
                Box::pin(async move {
                })
            }));

            d.on_message(Box::new(move |message: DataChannelMessage| {
                let msg_str = String::from_utf8(message.data.to_vec()).unwrap();
                let tx3 = tx2.clone();
                
                Box::pin(async move {
                    let _ = tx3.send(msg_str).await;
                })
            }));

            let self3 = self2.clone();

            Box::pin(async move {
                let mut x = self3.data_channel.lock().await;
                *x = Some(d.clone());
            })
        }));

        rx
    }

    pub async fn send_data_channel_message(&self, message: &String) {
        match &*self.data_channel.lock().await {
            Some(dc) => {
                dc.send_text(message).await;
            },
            None => {println!("Could not send message b/c no data channel is set!")}
        }
    }
}
