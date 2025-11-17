use crate::models::{
    ChannelName, DataChannelMap, DataRoomInternalType, SteckerData, SteckerDataChannel,
    SteckerDataChannelType,
};
use crate::utils::{decode_b64, encode_offer};

use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{error, info, instrument, trace, warn, Instrument, Span};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_remote::TrackRemote;

#[derive(Clone, Debug)]
pub enum ConnectionEvent {
    NewICEConnectionState(RTCIceConnectionState),
    NewPeerConnectionState(RTCPeerConnectionState),
}

/// This handles all the setup of a WebRTC peer connection.
pub struct SteckerWebRTCConnection {
    peer_connection: RTCPeerConnection,
    data_channel_map: Arc<Mutex<DataChannelMap>>,
    pub connection_events: Arc<Sender<ConnectionEvent>>,
}

impl SteckerWebRTCConnection {
    #[instrument]
    pub async fn build_connection() -> anyhow::Result<Self> {
        trace!("Build connection");
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

        let peer_connection = api.new_peer_connection(config).await?;

        let (connection_events_sender, _) = broadcast::channel::<ConnectionEvent>(4);
        let sender = Arc::new(connection_events_sender);

        let ice_span = Span::current();
        peer_connection.on_ice_connection_state_change(Box::new({
            let sender = Arc::clone(&sender);

            move |state| {
                let sender = Arc::clone(&sender);
                Box::pin(
                    async move {
                        trace!(?state, "New ICE connection state");
                        let _ = sender.send(ConnectionEvent::NewICEConnectionState(state));
                    }
                    .instrument(ice_span.clone()),
                )
            }
        }));

        let peer_span = Span::current();
        peer_connection.on_peer_connection_state_change(Box::new({
            let sender = Arc::clone(&sender);

            move |state| {
                let sender = Arc::clone(&sender);
                Box::pin(
                    async move {
                        trace!(?state, "New peer connection state");
                        let _ = sender.send(ConnectionEvent::NewPeerConnectionState(state));
                    }
                    .instrument(peer_span.clone()),
                )
            }
        }));

        Ok(Self {
            peer_connection,
            connection_events: sender,
            data_channel_map: Arc::new(Mutex::new(DataChannelMap(Mutex::new(HashMap::new())))),
        })
    }

    #[instrument(skip_all)]
    pub async fn respond_to_offer(&self, offer: String) -> anyhow::Result<String> {
        trace!("Responding to offer");
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

    #[instrument(skip_all, err)]
    pub async fn create_offer(&self) -> anyhow::Result<String> {
        trace!("Creating offer");
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
            Err(anyhow!("generate local_description failed!"))
        }
    }

    #[instrument(skip_all, err)]
    pub async fn set_remote_description(
        &self,
        description: RTCSessionDescription,
    ) -> anyhow::Result<()> {
        trace!("Set remove description");
        Ok(self
            .peer_connection
            .set_remote_description(description)
            .await?)
    }

    #[instrument(skip_all)]
    pub async fn close(&self) -> anyhow::Result<()> {
        trace!("Close stecker webrtc connection");
        Ok(self.peer_connection.close().await?)
    }

    #[instrument(skip_all)]
    fn connect_channel(
        data_channel: Arc<RTCDataChannel>,
        stecker_channel: Arc<SteckerDataChannel>,
        channel_type: SteckerDataChannelType,
    ) {
        trace!("Connect channel");
        let stecker_channel2 = stecker_channel.clone();
        let stecker_channel3 = stecker_channel.clone();
        let data_channel2 = data_channel.clone();

        data_channel.on_close(Box::new(move || {
            info!("Data channel closed");
            let _ = stecker_channel.close.send(());
            Box::pin(async {})
        }));

        let span = Span::current();

        data_channel.on_open(Box::new(move || {
            Box::pin(
                async move {
                    trace!("Started data channel connection thread");
                    let mut outbound_msg_rx = stecker_channel2.outbound.subscribe();
                    let mut close_rx = stecker_channel2.close.subscribe();

                    let mut _result = anyhow::Result::<usize>::Ok(0);

                    loop {
                        tokio::select! {
                            msg_to_send = outbound_msg_rx.recv() => {
                                match msg_to_send {
                                    Ok(msg) => {
                                        trace!(?msg, "Send out message");
                                        let _ = data_channel2.send(&msg.encode().unwrap()).await;
                                    },
                                    Err(_) => {
                                        // @todo we consume the queue as much as possible
                                        // this should be handled differently?
                                        while outbound_msg_rx.len() > 0 {
                                            let _ = outbound_msg_rx.recv().await;
                                        }
                                        warn!("Got a lagging error");
                                    },
                                };
                            },
                            _ = close_rx.recv() => {
                                info!("Received closing trigger");
                                break
                            }
                        }
                    }
                    info!("Stopped further consumption of the data channel");
                }
                .instrument(span),
            )
        }));

        data_channel.on_message(Box::new(move |message: DataChannelMessage| {
            trace!(?message, "Received data channel message");
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
    #[instrument(skip_all)]
    pub fn register_channel(&self, room_type: &DataRoomInternalType) -> Arc<SteckerDataChannel> {
        trace!("Register channel");
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
    #[instrument(skip_all)]
    pub async fn start_listening_for_data_channel(&self) {
        trace!("Start listening for channel");
        let span = Span::current().clone();

        let map = self.data_channel_map.clone();
        let _ = self
            .peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                span.in_scope(|| {
                    info!(label = d.label(), "Successfully listened for a channel");

                    if let Some(stecker_channel) = map.lock().unwrap().get(d.label()) {
                        let channel_type = stecker_channel.channel_type.clone();
                        Self::connect_channel(d, stecker_channel, channel_type);
                    } else {
                        warn!(
                            label = d.label(),
                            "Ignore data channel because label is not registered."
                        );
                    }
                });
                Box::pin(async {})
            }));
    }

    // we build data channel, other party has to listen
    #[instrument(skip_all)]
    pub async fn create_data_channel(
        &self,
        room_type: &DataRoomInternalType,
    ) -> anyhow::Result<SteckerDataChannel> {
        trace!("Create data channel");
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

    #[instrument(skip_all)]
    pub async fn create_audio_channel(&self) -> anyhow::Result<Arc<TrackLocalStaticSample>> {
        trace!("Create audio channel");
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 1,
                ..Default::default()
            },
            "audio".to_owned(),
            "stecker".to_owned(),
        ));

        let _ = self.peer_connection.add_track(audio_track.clone()).await?;

        Ok(audio_track)
    }

    #[instrument(skip_all)]
    pub async fn listen_for_remote_audio_track(&self) -> Receiver<Arc<TrackRemote>> {
        trace!("Listen for remote audio track");
        let (remote_track_tx, remote_track_rx) = tokio::sync::broadcast::channel(2);

        let _ = self
            .peer_connection
            .add_transceiver_from_kind(
                webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
                None,
            )
            .await;

        let span = Span::current();
        self.peer_connection.on_track(Box::new(move |track, _, _| {
            let _ = remote_track_tx.send(track);
            Box::pin(
                async {
                    trace!("Seen new track");
                }
                .instrument(span.clone()),
            )
        }));

        remote_track_rx
    }

    #[instrument(skip_all)]
    pub async fn add_existing_audio_track(&self, track: Arc<TrackLocalStaticRTP>) -> () {
        trace!("Add existing audio track");
        let _ = self.peer_connection.add_track(track).await;
        // maybe add this one as well
        // https://github.com/webrtc-rs/webrtc/blob/62f2550799efe2dd36cdc950ad3f334b120c75bb/examples/examples/broadcast/broadcast.rs#L258-L265
    }
}
