use crate::models::{RoomType, SteckerDataChannel, SteckerSendable};
use crate::utils::{decode_b64, encode_offer};

use std::sync::Arc;
use tokio::sync::broadcast;
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
    pub async fn listen_for_data_channel<T: SteckerSendable>(
        &self,
        room_type: &RoomType,
    ) -> SteckerDataChannel<T> {
        // messages received from data channel are inbound,
        // messages send to data channel are outbound
        let (inbound_msg_tx, _) = broadcast::channel::<T>(1024);
        let (outbound_msg_tx, _) = broadcast::channel::<T>(1024);

        let inbound_msg_tx2 = inbound_msg_tx.clone();
        let outbound_msg_tx2 = outbound_msg_tx.clone();

        let (close_trigger, _) = broadcast::channel::<()>(1);
        let close_trigger2 = close_trigger.clone();

        let room_type2 = room_type.clone();

        self.peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                let close_trigger3 = close_trigger2.clone();

                let label = d.label();
                let default_label = room_type2.get_default_label();
                if label != default_label {
                    println!("Ignore data channel because of mismatch in label: Is '{label}', should be '{default_label}'");
                    return Box::pin(async {})
                } else {
                    println!("Matched data channel {label}")
                }

                d.on_close(Box::new(move || {
                    println!("Data channel closed");
                    let _ = close_trigger3.send(());
                    Box::pin(async {})
                }));

                d.on_open(Box::new(move || {
                    println!("Opened channel");
                    Box::pin(async move {})
                }));

                let inbound_msg_tx3 = inbound_msg_tx2.clone();

                let d2 = d.clone();

                d.on_message(Box::new(move |message: DataChannelMessage| {
                    let msg = T::from_stecker_data(&message).unwrap();
                    // let msg = d2.convert_stecker_data(message).unwrap();
                    let _ = inbound_msg_tx3.send(msg);
                    Box::pin(async {})
                }));

                let mut outbound_msg_rx2 = outbound_msg_tx2.clone().subscribe();
                let mut close_trigger_receiver = close_trigger2.clone().subscribe();
                Box::pin(async move {
                    let mut consume = true;
                    tokio::spawn(async move {
                        let d2 = d.clone();
                        let mut _result = anyhow::Result::<usize>::Ok(0);

                        while consume {
                            tokio::select! {
                                msg_to_send = outbound_msg_rx2.recv() => {
                                    match msg_to_send {
                                        Ok(msg) => {
                                            let _ = d2.send(&msg.to_stecker_data().unwrap()).await;
                                        },
                                        Err(err) => {
                                            // @todo we consume the queue as much as possible
                                            // this should be handled differently?
                                            while outbound_msg_rx2.len() > 0 {
                                                let _ = outbound_msg_rx2.recv().await;
                                            }
                                            println!("Got a lagging error: {err}");
                                        },
                                    };
                                },
                                _ = close_trigger_receiver.recv() => {
                                    println!("Received closing trigger");
                                    consume = false;
                                }
                            }
                        }
                        println!("Stopped further consumption of the data channel");
                    });
                })
            }));

        SteckerDataChannel {
            outbound: outbound_msg_tx,
            inbound: inbound_msg_tx,
            close_trigger,
        }
    }

    // we build data channel, other party has to listen
    pub async fn create_data_channel<T: SteckerSendable>(
        &self,
        name: &str,
    ) -> anyhow::Result<SteckerDataChannel<T>> {
        // messages received from data channel are inbound,
        // messages send to data channel are outbound
        let (inbound_msg_tx, _) = broadcast::channel::<T>(1024);
        let (outbound_msg_tx, _) = broadcast::channel::<T>(1024);

        let inbound_msg_tx2 = inbound_msg_tx.clone();
        let outbound_msg_tx2 = outbound_msg_tx.clone();

        let data_channel = self.peer_connection.create_data_channel(name, None).await?;
        let d = data_channel.clone();

        let (close_trigger, _) = broadcast::channel::<()>(1);
        let close_trigger2 = close_trigger.clone();

        data_channel.on_open(Box::new(move || {
            println!("Opened data channel '{}'-'{}' open.", d.label(), d.id());

            let d2 = d.clone();
            let d3 = d.clone();
            d2.on_message(Box::new(move |message: DataChannelMessage| {
                let _ = inbound_msg_tx2.send(T::from_stecker_data(&message).unwrap());
                Box::pin(async {})
            }));

            let close_trigger3 = close_trigger2.clone();
            d2.on_close(Box::new(move || {
                println!("Close data channel");
                let _ = close_trigger3.clone().send(());
                Box::pin(async {})
            }));

            let close_trigger3 = close_trigger2.clone();

            Box::pin(async move {
                let d3 = d2.clone();
                let mut _result = anyhow::Result::<usize>::Ok(0);
                let mut consume = true;

                let mut receiver = outbound_msg_tx2.subscribe();
                let mut close_trigger_receiver = close_trigger3.clone().subscribe();
                while consume {
                    tokio::select! {
                        msg_to_send = receiver.recv() => {
                            match msg_to_send {
                                Ok(m) => {
                                    _result = d3.send(&m.to_stecker_data().unwrap()).await.map_err(Into::into)
                                },
                                Err(e) => {
                                    println!("Failed to send message to data channel: {e}");
                                },
                            }
                        },
                        _ = close_trigger_receiver.recv() => {
                            println!("Received stop signal");
                            consume = false;
                        }
                    }
                }
                println!("Stopped consuming messages");
            })
        }));

        Ok(SteckerDataChannel {
            outbound: outbound_msg_tx,
            inbound: inbound_msg_tx,
            close_trigger,
        })
    }
}
