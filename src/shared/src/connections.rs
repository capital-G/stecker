use crate::models::{ChannelName, DataChannelMap, GenericDataChannelMap, RoomType, SteckerDataChannel, SteckerSendable, SteckerDataChannelTrait};
use crate::utils::{decode_b64, encode_offer};

use std::collections::HashMap;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;
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

#[derive(Debug, Display)]
enum SteckerDataChannelEnum {
    Float(Arc<SteckerDataChannel<f32>>),
    String(Arc<SteckerDataChannel<String>>),
}

pub struct SteckerWebRTCConnection {
    peer_connection: RTCPeerConnection,
    // b/c of different mem size it is not easily possible
    // to store generic data in a single hashmap.
    // as the types are currently limited, we simply declare
    // them explicitly.
    string_data_channel_map: DataChannelMap<String>,
    float_data_channel_map: DataChannelMap<f32>,
    data_channel_map: DataChannelMap<SteckerDataChannelEnum>,
}

impl SteckerSendable for SteckerDataChannelEnum {
    fn to_stecker_data(&self) -> anyhow::Result<bytes::Bytes> {
        todo!()
    }

    fn from_stecker_data(data: &DataChannelMessage) -> anyhow::Result<Self> {
        todo!()
    }
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
            // data_channel: Mutex::new(None),
            // meta_channel: SteckerDataChannel::<String>::create_channels(),
            string_data_channel_map: DataChannelMap(Mutex::new(HashMap::new())),
            float_data_channel_map: DataChannelMap(Mutex::new(HashMap::new())),
            data_channel_map: GenericDataChannelMap(Mutex::new(HashMap::new())),
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

    fn connect_channel<T: SteckerSendable>(data_channel: Arc<RTCDataChannel>, stecker_channel: Arc<SteckerDataChannel<T>>) -> Pin<Box<(dyn Future<Output = ()> + Send + 'static)>> {
        let stecker_channel2 = stecker_channel.clone();
        let stecker_channel3 = stecker_channel.clone();

        data_channel.on_close(Box::new(move || {
            println!("Data channel closed");
            let _ = stecker_channel.close.send(());
            Box::pin(async {})
        }));

        data_channel.on_open(Box::new(move || {
            println!("Opened channel");
            Box::pin(async move {})
        }));

        data_channel.on_message(Box::new(move |message: DataChannelMessage| {
            let msg = T::from_stecker_data(&message).unwrap();
            // let msg = d2.convert_stecker_data(message).unwrap();
            let _ = stecker_channel3.inbound.send(msg);
            Box::pin(async {})
        }));

        Box::pin(async move {
            let mut outbound_msg_rx = stecker_channel2.outbound.subscribe();
            let mut close_rx = stecker_channel2.close.subscribe();

            tokio::spawn(async move {
                let data_channel2 = data_channel.clone();
                let mut _result = anyhow::Result::<usize>::Ok(0);

                loop {
                    tokio::select! {
                        msg_to_send = outbound_msg_rx.recv() => {
                            match msg_to_send {
                                Ok(msg) => {
                                    let _ = data_channel2.send(&msg.to_stecker_data().unwrap()).await;
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
            });
        })
    }

    pub fn register_data_channel<T: SteckerSendable>(&self, room_type: &RoomType) -> Arc<SteckerDataChannel<T>> {
        let stecker_channel = Arc::new(SteckerDataChannel::<T>::create_channels());
        let label = ChannelName::from(room_type);
        self.data_channel_map.0.lock().unwrap().insert(label, stecker_channel.clone());

        match room_type {
            RoomType::Float => {self.float_data_channel_map.0.lock().unwrap().insert(label, stecker_channel);},
            RoomType::Chat => {self.string_data_channel_map.0.lock().unwrap().insert(label, stecker_channel);},
            RoomType::Meta => todo!(),
        };
        stecker_channel
    }

    pub fn register_string_data_channel(&self, room_type: &RoomType) -> Arc<SteckerDataChannel<String>> {
        let stecker_channel = Arc::new(SteckerDataChannel::<String>::create_channels());
        self.string_data_channel_map.0.lock().unwrap().insert(ChannelName::from(room_type).to_string(), stecker_channel.clone());
        stecker_channel
    }

    pub fn register_float_data_channel(&self, room_type: &RoomType) -> Arc<SteckerDataChannel<f32>> {
        let stecker_channel = Arc::new(SteckerDataChannel::<f32>::create_channels());
        self.float_data_channel_map.0.lock().unwrap().insert(ChannelName::from(room_type).to_string(), stecker_channel.clone());
        stecker_channel
    }

    // pub fn register_data_channel<T: SteckerSendable>(self, room_type: &RoomType) -> Arc<SteckerDataChannel<T>> {
    //     let stecker_channel = Arc::new(SteckerDataChannel::<T>::create_channels());
    //     stecker_channel
    // }

    // other party builds data channel and we listen for it
    // should only be called once.
    pub async fn start_listening_for_data_channel(self) {
        self.peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                if let Some(stecker_channel) = self.string_data_channel_map.get(d.label()) {
                    Self::connect_channel(d, stecker_channel)
                }
                else if let Some(stecker_channel) = self.float_data_channel_map.get(d.label()) {
                    Self::connect_channel(d, stecker_channel)
                }
                else {
                    println!("Ignore data channel '{}' because label is not registered.", d.label());
                    Box::pin(async {})
                }
            }));
    }

    fn connect_create_channel<T: SteckerSendable>(data_channel: Arc<RTCDataChannel>, stecker_channel: SteckerDataChannel<T>) {
        let data_channel2 = data_channel.clone();
        data_channel.on_open(Box::new(move || {
            let stecker_channel2 = stecker_channel.clone();
            let stecker_channel3 = stecker_channel.clone();

            println!("Opened data channel '{}'-'{}' open.", data_channel2.label(), data_channel2.id());

            data_channel2.on_message(Box::new(move |message: DataChannelMessage| {
                let _ = stecker_channel2.inbound.send(T::from_stecker_data(&message).unwrap());
                Box::pin(async {})
            }));

            data_channel2.on_close(Box::new(move || {
                println!("Close data channel");
                let _ = stecker_channel2.close.send(());
                Box::pin(async {})
            }));

            Box::pin(async move {
                let data_channel3 = data_channel2.clone();
                let mut _result = anyhow::Result::<usize>::Ok(0);

                let mut receiver = stecker_channel3.outbound.subscribe();
                let mut close_trigger_receiver = stecker_channel3.close.subscribe();
                loop {
                    tokio::select! {
                        msg_to_send = receiver.recv() => {
                            match msg_to_send {
                                Ok(m) => {
                                    _result = data_channel3.send(&m.to_stecker_data().unwrap()).await.map_err(Into::into)
                                },
                                Err(e) => {
                                    println!("Failed to send message to data channel: {e}");
                                },
                            }
                        },
                        _ = close_trigger_receiver.recv() => {
                            println!("Received stop signal");
                            break
                        }
                    }
                }
                println!("Stopped consuming messages");
            })
        }))
    }

    // we build data channel, other party has to listen
    pub async fn create_data_channel<T: SteckerSendable>(
        &self,
        room_type: &RoomType,
    ) -> anyhow::Result<SteckerDataChannel<T>> {
        let stecker_channel = SteckerDataChannel::<T>::create_channels();
        let stecker_channel2 = stecker_channel.clone();

        let data_channel = self.peer_connection.create_data_channel(&ChannelName::from(room_type), None).await?;

        Self::connect_create_channel(data_channel, stecker_channel);

        Ok(stecker_channel2)
    }
}

// trait RegisterDataChannel {
//     fn register_data_channel(&self,room_type: &RoomType) -> Arc<SteckerDataChannel<Self>>
//     where
//         Self: SteckerSendable;
// }

// impl RegisterDataChannel for f32 {
//     fn register_data_channel(&self, room_type: &RoomType) -> Arc<SteckerDataChannel<f32>> {
//         let stecker_channel = Arc::new(SteckerDataChannel::<f32>::create_channels());
//         self.map.0.lock().unwrap().insert(ChannelName::from(room_type).to_string(), stecker_channel.clone());
//         stecker_channel
//     }
// }
