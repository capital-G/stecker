use base64::Engine;
use tokio::sync::Mutex;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use crate::models::{BroadcastRoom, Connection, User};
use anyhow::anyhow;

use base64::prelude::BASE64_STANDARD;

use std::sync::Arc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;

pub fn encode_offer(offer: RTCSessionDescription) -> anyhow::Result<String> {
    let json = serde_json::to_string(&offer)?;
    Ok(BASE64_STANDARD.encode(json))
}

pub fn decode_b64(s: &str) -> anyhow::Result<String> {
    let b = BASE64_STANDARD.decode(s)?;

    let s = String::from_utf8(b)?;
    Ok(s)
}

pub struct ConnectionWithOffer {
    pub connection: Connection,
    pub offer: String,
}

async fn handle_message(connection: &Connection, message: DataChannelMessage) {
    println!("Its time to handle the message");
}

impl Connection {
    pub async fn respond_to_offer(offer: String) -> anyhow::Result<ConnectionWithOffer> {
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

        peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                let d_label = d.label().to_owned();
                let d_id = d.id();
                let d2 = d.clone();

                d.on_close(Box::new(|| {
                    println!("Data channel closed");
                    Box::pin(async {})
                }));

                d.on_open(Box::new(move || {
                    println!("Opened channel");
                    Box::pin(async {
                        // hello_world().await;
                    })
                }));

                d.on_message(Box::new(move |message: DataChannelMessage| {
                    let msg_str = String::from_utf8(message.data.to_vec()).unwrap();
                    println!("Received message {msg_str}");
                    let msg2 = msg_str.clone();
                    let d2 = d2.clone();
                    Box::pin(async move {
                        let _: Result<usize, webrtc::Error> = d2.send_text(msg2).await;
                        // --> here I want to call a function located in connection: Connection
                    })
                }));
                Box::pin(async {})
            }
        ));

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

        let connection = Connection {
            peer_connection,
            data_channel: None,
        };

        Ok(ConnectionWithOffer{
            connection,
            offer: offer?
        })
    }

    pub async fn received_message(&self, message: DataChannelMessage) {

    }

    // pub async fn create_offer(&self) -> anyhow::Result<String> {
    //     // Create an offer to send to the browser
    //     let offer = &self.peer_connection.create_offer(None).await?;

    //     // Create channel that is blocked until ICE Gathering is complete
    //     let mut gather_complete = &self.peer_connection.gathering_complete_promise().await;

    //     // Sets the LocalDescription, and starts our UDP listeners
    //     let _ = &self.peer_connection.set_local_description(offer.clone()).await?;

    //     match self.peer_connection.local_description().await {
    //         None => Err(anyhow!("Could not create local description!")),
    //         Some(description) => encode_offer(description)
    //     } 
    // }   
}

impl BroadcastRoom {
    pub async fn create_new_room(name: String) -> anyhow::Result<Self> {
        // let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

        // peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        //     println!("Peer connection state changed to {}", s);

        //     if s == RTCPeerConnectionState::Failed {
        //         println!("Peer connection failed: {}", s);
        //         let _ = done_tx.try_send(());
        //     }

        //     Box::pin(async {})
        // }));

        let mut broadcast_room = BroadcastRoom {
            name,
            source_connection: None,
            target_connections: Mutex::new(vec![]),
        };

        Ok(broadcast_room)

    }

    pub async fn join_room(&mut self, connection: Connection) {
        let conns = self.target_connections.get_mut();
        conns.push(connection);
    }
}
