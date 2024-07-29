use crate::models::{
    ChannelName, DataChannelMap, RoomType, SteckerData, SteckerDataChannel, SteckerDataChannelType,
};
use crate::utils::{decode_b64, encode_offer};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
    peer_connection: RTCPeerConnection,
    data_channel_map: Arc<Mutex<DataChannelMap>>,
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
            data_channel_map: Arc::new(Mutex::new(DataChannelMap(Mutex::new(HashMap::new())))),
            // float_data_channel_map: DataChannelMap(Mutex::new(HashMap::new())),
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

    fn connect_channel(
        data_channel: Arc<RTCDataChannel>,
        stecker_channel: Arc<SteckerDataChannel>,
        channel_type: SteckerDataChannelType,
    ) {
        let stecker_channel2 = stecker_channel.clone();
        let stecker_channel3 = stecker_channel.clone();
        let data_channel2 = data_channel.clone();

        data_channel.on_close(Box::new(move || {
            println!("Data channel closed");
            let _ = stecker_channel.close.send(());
            Box::pin(async {})
        }));

        data_channel.on_open(Box::new(move || {
            Box::pin(async move {
                let mut outbound_msg_rx = stecker_channel2.outbound.subscribe();
                let mut close_rx = stecker_channel2.close.subscribe();

                let mut _result = anyhow::Result::<usize>::Ok(0);

                loop {
                    tokio::select! {
                        msg_to_send = outbound_msg_rx.recv() => {
                            match msg_to_send {
                                Ok(msg) => {
                                    // println!("Send out message: {msg}");
                                    let _ = data_channel2.send(&msg.encode().unwrap()).await;
                                },
                                Err(err) => {
                                    // @todo we consume the queue as much as possible
                                    // this should be handled differently?
                                    while outbound_msg_rx.len() > 0 {
                                        let _ = outbound_msg_rx.recv().await;
                                    }
                                    println!("Got a lagging error: {err}");
                                },
                            };
                        },
                        _ = close_rx.recv() => {
                            println!("Received closing trigger");
                            break
                        }
                    }
                }
                println!("Stopped further consumption of the data channel");
            })
        }));

        data_channel.on_message(Box::new(move |message: DataChannelMessage| {
            let msg = match channel_type {
                SteckerDataChannelType::Float => SteckerData::decode_float(message),
                SteckerDataChannelType::String => SteckerData::decode_string(message),
            };
            // @todo unwrap is dangerous here
            let _ = stecker_channel3.inbound.send(msg.unwrap());
            Box::pin(async {})
        }));
    }

    /// Used if we "listen" for a data channel from the other site.
    /// this will return the tokio-channels and will put these channels
    /// also into an internal hashmap under the name of the room_type,
    /// which will me looked up and wired up if the data channel
    /// appears during listening.
    ///
    /// Remember to call start_listening_for_data_channel to start listening
    /// for channels from the other side.
    pub fn register_channel(&self, room_type: &RoomType) -> Arc<SteckerDataChannel> {
        let stecker_channel = Arc::new(SteckerDataChannel::create_channels(
            SteckerDataChannelType::from(room_type.clone()),
        ));
        let label = ChannelName::from(room_type);
        self.data_channel_map
            .lock()
            .unwrap()
            .insert(&label, stecker_channel.clone());
        stecker_channel
    }

    // other party builds data channel and we listen for it
    // should only be called once.
    pub async fn start_listening_for_data_channel(&self) {
        let map = self.data_channel_map.clone();
        self.peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                // println!("Successfully listened for channel {}", d.label());
                if let Some(stecker_channel) = map.lock().unwrap().get(d.label()) {
                    let channel_type = stecker_channel.channel_type.clone();
                    Self::connect_channel(d, stecker_channel, channel_type);
                    Box::pin(async {})
                } else {
                    println!(
                        "Ignore data channel '{}' because label is not registered.",
                        d.label()
                    );
                    Box::pin(async {})
                }
            }));
    }

    // we build data channel, other party has to listen
    pub async fn create_data_channel(
        &self,
        room_type: &RoomType,
    ) -> anyhow::Result<SteckerDataChannel> {
        let stecker_channel =
            SteckerDataChannel::create_channels(SteckerDataChannelType::from(room_type.clone()));
        let stecker_channel2 = Arc::new(stecker_channel.clone());

        let data_channel = self
            .peer_connection
            .create_data_channel(&ChannelName::from(room_type), None)
            .await?;

        let _ = Self::connect_channel(
            data_channel,
            stecker_channel2,
            SteckerDataChannelType::from(room_type.clone()),
        );

        Ok(stecker_channel)
    }
}
