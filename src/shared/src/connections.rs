use crate::utils::{decode_b64, encode_offer};

use tokio::sync::broadcast;
use tokio::sync::Mutex;
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

pub struct SteckerWebRTCConnection {
    // FIXME: potentially both fields are obsolete
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

    pub async fn send_data_channel_message(&self, message: &String) {
        match &*self.data_channel.lock().await {
            Some(dc) => {
                let _ = dc.send_text(message).await;
            }
            None => {
                println!("Could not send message b/c no data channel is set!")
            }
        }
    }

    pub async fn listen_for_data_channel(
        &self,
    ) -> (broadcast::Sender<String>, broadcast::Sender<String>) {
        let (in_tx, _we_do_not_receive_here) = broadcast::channel::<String>(1);
        let (out_tx, _out_rx) = broadcast::channel::<String>(1);
        let out_tx_clone = out_tx.clone();
        let in_tx_clone = in_tx.clone();

        self.peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                let tx2 = in_tx.clone();
                let out_tx2 = out_tx.clone();

                d.on_close(Box::new(|| {
                    println!("Data channel closed");
                    Box::pin(async {})
                }));

                d.on_open(Box::new(move || {
                    println!("Opened channel");
                    Box::pin(async move {})
                }));

                d.on_message(Box::new(move |message: DataChannelMessage| {
                    let msg_str = String::from_utf8(message.data.to_vec()).unwrap();
                    let tx3 = tx2.clone();

                    Box::pin(async move {
                        let _ = tx3.send(msg_str);
                    })
                }));

                Box::pin(async move {
                    tokio::spawn(async move {
                        let mut out_rx2 = out_tx2.subscribe();

                        while let Ok(msg) = out_rx2.recv().await {
                            println!("Sending out something: {}", msg.clone());
                            match d.send_text(msg).await {
                                Ok(_) => {}
                                Err(err) => {
                                    println!(
                                    "ERROR: forwarding message from channel to data_channel {err}"
                                );
                                }
                            }
                        }
                    });
                })
            }));

        (out_tx_clone, in_tx_clone)
    }
}
