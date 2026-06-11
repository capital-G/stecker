use crate::models::{SteckerChannelType, SteckerData, SteckerDataChanelTrait, SteckerDataChannel};
use crate::utils::{decode_b64, encode_offer};

use anyhow::anyhow;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Sender};
use tracing::{error, instrument, trace, Instrument, Span};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice::network_type::NetworkType;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::signaling_state::RTCSignalingState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType};
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_remote::TrackRemote;

#[derive(Clone)]
pub enum ConnectionEvent {
    NewICEConnectionState(RTCIceConnectionState),
    NewPeerConnectionState(RTCPeerConnectionState),
    NewSignalState(RTCSignalingState),
    NewDataChannel(Arc<RTCDataChannel>),
    NewAudioChannel(Arc<TrackRemote>),
}

impl Debug for ConnectionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewICEConnectionState(arg0) => {
                f.debug_tuple("NewICEConnectionState").field(arg0).finish()
            }
            Self::NewPeerConnectionState(arg0) => {
                f.debug_tuple("NewPeerConnectionState").field(arg0).finish()
            }
            Self::NewDataChannel(_) => f.debug_tuple("NewDataChannel").finish(),
            Self::NewAudioChannel(arg0) => f.debug_tuple("NewAudioChannel").field(arg0).finish(),
            Self::NewSignalState(arg0) => f
                .debug_tuple("ConnectionEvent::NewSignalState")
                .field(arg0)
                .finish(),
        }
    }
}

/// This handles all the setup of a WebRTC peer connection.
pub struct SteckerWebRTCConnection {
    peer_connection: RTCPeerConnection,
    // data_channel_map: Arc<Mutex<DataChannelMap>>,
    pub connection_events: broadcast::Sender<ConnectionEvent>,
    connection_events_rx: broadcast::Receiver<ConnectionEvent>,
}

impl SteckerWebRTCConnection {
    /// Pass a connection events such that it is possible to define what should happen
    /// with the connection before the connection is made.
    #[instrument]
    pub async fn build_connection(
        connection_events: Sender<ConnectionEvent>,
    ) -> anyhow::Result<Self> {
        trace!("Build connection");
        let connection_events_rx = connection_events.subscribe();
        let mut m = MediaEngine::default();
        m.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut m)?;

        // let mut settings_engine = SettingEngine::default();
        // settings_engine.set_network_types(vec![NetworkType::Udp4]);

        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            // .with_setting_engine(settings_engine)
            .build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer_connection = api.new_peer_connection(config).await?;

        let sender = connection_events.clone();
        let ice_span = Span::current();
        peer_connection.on_ice_connection_state_change(Box::new({
            move |state| {
                let sender = sender.clone();
                Box::pin(
                    async move {
                        // trace!(?state, "New ICE connection state");
                        let _ = sender.send(ConnectionEvent::NewICEConnectionState(state));
                    }
                    .instrument(ice_span.clone()),
                )
            }
        }));

        let sender = connection_events.clone();
        let signal_span = Span::current();
        peer_connection.on_signaling_state_change(Box::new(|signal| Box::pin(async {})));

        let sender = connection_events.clone();
        let peer_span = Span::current();
        peer_connection.on_peer_connection_state_change(Box::new({
            move |state| {
                let sender = sender.clone();
                Box::pin(
                    async move {
                        // trace!(?state, "New peer connection state");
                        let _ = sender.send(ConnectionEvent::NewPeerConnectionState(state));
                    }
                    .instrument(peer_span.clone()),
                )
            }
        }));

        let sender = connection_events.clone();
        let data_span = Span::current();
        let _ = peer_connection
            .on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
                data_span.in_scope(|| {
                    let label = channel.label();
                    trace!(label, "Seen new data channel");
                    let _ = sender.send(ConnectionEvent::NewDataChannel(channel));
                });
                Box::pin(async {})
            }))
            .in_current_span();

        let sender = connection_events.clone();
        // let _ = peer_connection.add_transceiver_from_kind(webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio, None).await;

        let track_span = Span::current();
        let _ = peer_connection
            .on_track(Box::new(move |track, _, _| {
                track_span.in_scope(|| {
                    trace!("Seen new rtp track");
                    let _ = sender.send(ConnectionEvent::NewAudioChannel(track));
                });
                Box::pin(async {})
            }))
            .in_current_span();

        Ok(Self {
            peer_connection,
            connection_events,
            connection_events_rx,
        })
    }

    #[instrument(skip_all)]
    pub async fn respond_to_offer(&self, offer: &String) -> anyhow::Result<String> {
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
        trace!("Set remote description");
        Ok(self
            .peer_connection
            .set_remote_description(description)
            .await?)
    }

    /// Closes the connection - remember to call this or otherwise
    /// there will be a dangling socket.
    #[instrument(skip_all)]
    pub async fn close(&self) -> anyhow::Result<()> {
        trace!("Close stecker webrtc connection");
        Ok(self.peer_connection.close().await?)
    }

    /// Waits for a channel to receive from the other side.
    /// This is blocking until a channel is matched!
    #[instrument(skip_all)]
    pub async fn wait_for_data_channel<T>(&self) -> Arc<RTCDataChannel>
    where
        T: SteckerData + SteckerChannelType,
        SteckerDataChannel<T>: SteckerDataChanelTrait,
    {
        let mut events = self.connection_events.subscribe();
        loop {
            if let Ok(ConnectionEvent::NewDataChannel(data_channel)) = events.recv().await {
                if T::label() == data_channel.label() {
                    trace!("Matched data channel");
                    return data_channel;
                }
            }
        }
    }

    /// Waits for a channel to receive from the other side.
    /// This is blocking until a channel is matched!
    #[instrument(skip_all)]
    pub async fn wait_for_audio_channel(&self) -> Arc<TrackRemote> {
        let mut events = self.connection_events.subscribe();
        loop {
            if let Ok(ConnectionEvent::NewAudioChannel(audio_channel)) = events.recv().await {
                trace!("Found audio channel");
                return audio_channel;
            }
        }
    }

    #[instrument(skip_all)]
    pub async fn wait_for_disconnect(&self) -> () {
        let mut events = self.connection_events.subscribe();
        loop {
            if let Ok(ConnectionEvent::NewPeerConnectionState(
                RTCPeerConnectionState::Disconnected,
            )) = events.recv().await
            {
                trace!("Connection got closed");
                return ();
            }
        }
    }

    /// We build the data channel, the other party has to listen
    /// for the data channel using `connect_channel`
    #[instrument(skip_all)]
    pub async fn create_data_channel<T>(
        &self,
        stecker_channel: Arc<SteckerDataChannel<T>>,
    ) -> anyhow::Result<()>
    where
        T: SteckerData + SteckerChannelType,
        SteckerDataChannel<T>: SteckerDataChanelTrait,
    {
        match self
            .peer_connection
            .create_data_channel(T::label().as_str(), None)
            .await
        {
            Ok(data_channel) => {
                let _ = stecker_channel.connect(&data_channel).await;
                Ok(())
            }
            Err(err) => {
                error!(?err, "Failed to create data channel");
                anyhow::bail!("Failed to create data channel");
            }
        }
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
    pub async fn add_recvonly_audio_transceiver(&self) -> anyhow::Result<()> {
        trace!("Add recvonly audio transceiver");
        self.peer_connection
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await?;
        Ok(())
    }

    #[instrument(skip_all)]
    pub async fn add_existing_audio_track(&self, track: Arc<TrackLocalStaticRTP>) -> () {
        trace!("Add existing audio track");
        let _ = self.peer_connection.add_track(track).await;
        // maybe add this one as well
        // https://github.com/webrtc-rs/webrtc/blob/62f2550799efe2dd36cdc950ad3f334b120c75bb/examples/examples/broadcast/broadcast.rs#L258-L265
    }

    #[instrument(skip_all)]
    pub async fn forward_messages<T>(
        &self,
        stecker_channel: &Arc<SteckerDataChannel<T>>,
    ) -> anyhow::Result<()>
    where
        T: SteckerData,
        SteckerDataChannel<T>: SteckerDataChanelTrait,
    {
        todo!()
    }
}
