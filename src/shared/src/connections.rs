use crate::utils::{decode_b64, encode_offer};

use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
use tokio::sync::Mutex;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

pub struct SteckerDataChannel {
    pub inbound: Sender<String>,
    pub outbound: Sender<String>,
}

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

    pub async fn create_offer(&self) -> anyhow::Result<String> {
        // Create an offer to send to the browser
        let offer = self.peer_connection.create_offer(None).await?;

        // Create channel that is blocked until ICE Gathering is complete
        let mut gather_complete = self.peer_connection.gathering_complete_promise().await;

        // Sets the LocalDescription, and starts our UDP listeners
        self.peer_connection.set_local_description(offer).await?;

        // Block until ICE Gathering is complete, disabling trickle ICE
        // we do this because we only can exchange one signaling message
        // in a production application you should exchange ICE Candidates via OnICECandidate
        let _ = gather_complete.recv().await;

        // Output the answer in base64 so we can safely transfer it as a json value
        if let Some(local_desc) = self.peer_connection.local_description().await {
            let b64 = encode_offer(local_desc)?;
            Ok(b64)
        } else {
            println!("generate local_description failed!");
            Err(anyhow::Error::msg("generate local_description failed!"))
        }
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

    pub async fn set_remote_description(
        &self,
        description: RTCSessionDescription,
    ) -> anyhow::Result<()> {
        Ok(self
            .peer_connection
            .set_remote_description(description)
            .await?)
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        Ok(self.peer_connection.close().await?)
    }

    // other party builds data channel and we listen for it
    pub async fn listen_for_data_channel(&self) -> SteckerDataChannel {
        // messages received from data channel are inbound,
        // messages send to data channel are outbound
        let (inbound_msg_tx, _) = broadcast::channel::<String>(2);
        let (outbound_msg_tx, _) = broadcast::channel::<String>(2);

        let inbound_msg_tx2 = inbound_msg_tx.clone();
        let outbound_msg_tx2 = outbound_msg_tx.clone();

        self.peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                d.on_close(Box::new(|| {
                    println!("Data channel closed");
                    Box::pin(async {})
                }));

                d.on_open(Box::new(move || {
                    println!("Opened channel");
                    Box::pin(async move {})
                }));

                let inbound_msg_tx3 = inbound_msg_tx2.clone();

                d.on_message(Box::new(move |message: DataChannelMessage| {
                    let msg_str = String::from_utf8(message.data.to_vec()).unwrap();
                    let _ = inbound_msg_tx3.send(msg_str);
                    Box::pin(async {})
                }));

                let mut outbound_msg_rx2 = outbound_msg_tx2.clone().subscribe();

                Box::pin(async move {
                    // if we do not spawn here the data connection will not get picked up!
                    tokio::spawn(async move {
                        let d2 = d.clone();
                        let mut result = anyhow::Result::<usize>::Ok(0);

                        while result.is_ok() {
                            tokio::select! {
                                Ok(msg_to_send) = outbound_msg_rx2.recv() => {
                                    result = d2.send_text(msg_to_send).await.map_err(Into::into);
                                },
                            }
                        }
                    });
                })
            }));

        SteckerDataChannel {
            outbound: outbound_msg_tx,
            inbound: inbound_msg_tx,
        }
    }

    // we build data channel, other party has to listen
    pub async fn create_data_channel(&self, name: &str) -> anyhow::Result<SteckerDataChannel> {
        // messages received from data channel are inbound,
        // messages send to data channel are outbound
        let (inbound_msg_tx, _) = broadcast::channel::<String>(2);
        let (outbound_msg_tx, _) = broadcast::channel::<String>(2);

        let inbound_msg_tx2 = inbound_msg_tx.clone();
        let outbound_msg_tx2 = outbound_msg_tx.clone();

        let data_channel = self.peer_connection.create_data_channel(name, None).await?;
        let d = data_channel.clone();

        data_channel.on_open(Box::new(move || {
            println!("Opened data channel '{}'-'{}' open.", d.label(), d.id());

            let d2 = d.clone();
            d2.on_message(Box::new(move |msg: DataChannelMessage| {
                let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                let _ = inbound_msg_tx2.send(msg_str);
                Box::pin(async {})
            }));

            Box::pin(async move {
                let d3 = d2.clone();
                let mut result = anyhow::Result::<usize>::Ok(0);

                let mut recveiver = outbound_msg_tx2.subscribe();

                while result.is_ok() {
                    tokio::select! {
                        msg_to_send = recveiver.recv() => {
                            match msg_to_send {
                                Ok(m) => {
                                    println!("I will now send out {m}");
                                    result = d3.send_text(m).await.map_err(Into::into);
                                },
                                Err(e) => {
                                    println!("Failed to send message to data channel: {e}");
                                },
                            }

                        }
                    }
                }
            })
        }));

        Ok(SteckerDataChannel {
            outbound: outbound_msg_tx,
            inbound: inbound_msg_tx,
        })
    }
}
