use crate::event_service::SteckerConnectionEvent;
use crate::models::{
    ChannelName, DataChannelMap, DataRoomInternalType, SteckerData, SteckerDataChannel,
    SteckerDataChannelEvent, SteckerDataChannelType,
};
use crate::utils::{decode_b64, encode_offer};

use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast::{self, Sender};
use tracing::{debug, info, instrument, trace, warn, Instrument, Span};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
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

/// This handles all the setup of a WebRTC peer connection.
#[derive(Clone)]
pub struct SteckerWebRTCConnection {
    peer_connection: Arc<RTCPeerConnection>,
    // WebRTC connections can multiplex data channels
    // in order to differentiate between them we store them in a map.
    data_channel_map: Arc<RwLock<DataChannelMap>>,
    pub event_service: Sender<SteckerConnectionEvent>,
}

impl SteckerWebRTCConnection {
    #[instrument]
    /// sets up a new WebRTC connection
    pub async fn build_connection() -> anyhow::Result<Self> {
        let (event_service_rx, _) = broadcast::channel::<SteckerConnectionEvent>(16);

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

        let connection_event_service = event_service_rx.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            let _ = connection_event_service
                .send(SteckerConnectionEvent::ConnectionStateChanged(state));
            trace!(?state, "New connection state");
            Box::pin(async move {})
        }));

        let connection = Self {
            event_service: event_service_rx,
            peer_connection: Arc::new(peer_connection),
            data_channel_map: Arc::new(RwLock::new(DataChannelMap(RwLock::new(HashMap::new())))),
        };

        // is this a good thing? Will yield data channels and audio tracks to our event bus
        connection.start_listening_for_data_channel().await;
        connection.start_listening_for_audio_track().await;

        Ok(connection)
    }

    /// When offered a WebRTC connection we must respond also with an offer string which this function generates.
    /// After a successful handshake the peer connection can be used.
    /// This function is used by the server.
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

        if let Some(local_desc) = self.peer_connection.local_description().await {
            encode_offer(local_desc)
        } else {
            Err(anyhow::anyhow!("Error while creating RTC offer"))
        }
    }

    #[instrument(skip_all, err)]
    /// A WebRTC connection is peer to peer which requires handshakes from both parties via an offer.
    /// This function creates an offer from the sending site and is used by the clients.
    pub async fn create_offer(&self) -> anyhow::Result<String> {
        // Create an offer to send to the other side
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
            encode_offer(local_desc)
        } else {
            Err(anyhow!("generate local_description failed!"))
        }
    }

    #[instrument(skip_all)]
    /// this acts as a callback for the data channel listener.
    /// this connects the data channel to the stecker channel.
    fn connect_channel(
        &self,
        data_channel: Arc<RTCDataChannel>,
        stecker_channel: Arc<SteckerDataChannel>,
        channel_type: SteckerDataChannelType,
    ) {
        let close_sender = stecker_channel.events.clone();
        data_channel.on_close(Box::new(move || {
            info!("Data channel closed");
            let _ = close_sender.send(SteckerDataChannelEvent::ConnectionClosed);
            Box::pin(async {})
        }));

        let mut outbound_sender = stecker_channel.events.subscribe();
        let data_channel2 = data_channel.clone();
        data_channel.on_open(Box::new(move || {
            Box::pin(
                async move {
                    loop {
                        match outbound_sender.recv().await {
                            Ok(event) => match event {
                                SteckerDataChannelEvent::OutboundMessage(stecker_data) => {
                                    let _ =
                                        data_channel2.send(&stecker_data.encode().unwrap()).await;
                                }
                                SteckerDataChannelEvent::ConnectionClosed => break,
                                _ => {}
                            },
                            Err(err) => {
                                warn!(?err, "Consuming out sender did not work properly!");
                                break;
                            }
                        }
                    }
                    debug!("Stop consuming outbound sender");
                }
                .in_current_span(),
            )
        }));

        // let message_channel = self.event_service.clone();
        let inbound_receiver = stecker_channel.events.clone();
        data_channel.on_message(Box::new(move |message: DataChannelMessage| {
            match channel_type {
                SteckerDataChannelType::Float => {
                    if let Ok(data) = SteckerData::decode_float(message) {
                        let _ =
                            inbound_receiver.send(SteckerDataChannelEvent::InboundMessage(data));
                    }
                }
                SteckerDataChannelType::String => {
                    if let Ok(data) = SteckerData::decode_string(message) {
                        let _ =
                            inbound_receiver.send(SteckerDataChannelEvent::InboundMessage(data));
                    }
                }
            };
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
    pub fn register_channel(&self, room_type: &DataRoomInternalType) -> Arc<SteckerDataChannel> {
        let stecker_channel = Arc::new(SteckerDataChannel::create_channels(
            SteckerDataChannelType::from(room_type.clone()),
        ));
        let label = ChannelName::from(room_type);
        self.data_channel_map
            .write()
            .unwrap()
            .insert(&label, stecker_channel.clone());
        stecker_channel
    }

    /// sets up a listener for a data channel - each data channel will be
    /// published via `event_service`.
    /// !! should only be called once !!
    async fn start_listening_for_data_channel(&self) {
        let new_data_channel_sender = self.event_service.clone();
        self.peer_connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                // @todo instrument this!
                trace!(label = d.label(), "Received new data channel");
                let _ = new_data_channel_sender.send(SteckerConnectionEvent::NewDataChannel(d));
                Box::pin(async {})
            }));
    }

    /// creates a data channel The other party has to listen for this channel
    // @todo this should be reworked?
    pub async fn create_data_channel(
        &self,
        room_type: &DataRoomInternalType,
    ) -> anyhow::Result<Arc<SteckerDataChannel>> {
        let stecker_channel = Arc::new(SteckerDataChannel::create_channels(
            SteckerDataChannelType::from(room_type.clone()),
        ));
        let data_channel = self
            .peer_connection
            .create_data_channel(&ChannelName::from(room_type), None)
            .await?;

        self.connect_channel(
            data_channel,
            stecker_channel.clone(),
            SteckerDataChannelType::from(room_type.clone()),
        );

        Ok(stecker_channel)
    }

    /// create an audio channel on the peer connection which we use to pass/broadcast
    /// the audio track from an existing sender
    pub async fn create_audio_channel(&self) -> anyhow::Result<Arc<TrackLocalStaticSample>> {
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

    /// will listen for incoming audio tracks which will then be published
    /// via the event_service.
    async fn start_listening_for_audio_track(&self) {
        let _ = self
            .peer_connection
            .add_transceiver_from_kind(
                webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
                None,
            )
            .await;

        let audio_track_sender = self.event_service.clone();
        self.peer_connection.on_track(Box::new(move |track, _, _| {
            let _ = audio_track_sender.send(SteckerConnectionEvent::NewAudioTrack(track));
            Box::pin(async {})
        }));
    }

    /// add an audio track to the WebRTC stream
    pub async fn add_existing_audio_track(&self, track: Arc<TrackLocalStaticRTP>) -> () {
        let _ = self.peer_connection.add_track(track).await;
        // maybe add this one as well
        // https://github.com/webrtc-rs/webrtc/blob/62f2550799efe2dd36cdc950ad3f334b120c75bb/examples/examples/broadcast/broadcast.rs#L258-L265
    }

    pub async fn wait_for_audio_track(&self) -> Option<Arc<TrackRemote>> {
        let mut event_receiver = self.event_service.subscribe();
        loop {
            if let Ok(event) = event_receiver.recv().await {
                match event {
                    SteckerConnectionEvent::NewAudioTrack(audio_track) => {
                        return Some(audio_track);
                    }
                    SteckerConnectionEvent::ConnectionStateChanged(
                        RTCPeerConnectionState::Closed,
                    ) => {
                        return None;
                    }
                    _ => {}
                }
            }
        }
    }

    pub async fn wait_for_data_channel(
        &self,
        channel_type: &SteckerDataChannelType,
    ) -> Option<Arc<RTCDataChannel>> {
        let mut event_receiver = self.event_service.subscribe();
        loop {
            if let Ok(event) = event_receiver.recv().await {
                match event {
                    SteckerConnectionEvent::NewDataChannel(data_channel) => {
                        if data_channel.label()
                            == ChannelName::from(&DataRoomInternalType::from(*channel_type))
                        {
                            return Some(data_channel);
                        }
                    }
                    SteckerConnectionEvent::ConnectionStateChanged(
                        RTCPeerConnectionState::Closed,
                    ) => {
                        return None;
                    }
                    _ => {}
                }
            }
        }
    }
}
