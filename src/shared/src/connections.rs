use crate::models::{
    ChannelName, DataChannelMap, DataRoomType, SteckerAudioChannel, SteckerData,
    SteckerDataChannel, SteckerDataChannelType,
};
use crate::utils::{decode_b64, encode_offer};

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::io::ogg_reader::OggReader;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_local::TrackLocalWriter;

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
    pub fn register_channel(&self, room_type: &DataRoomType) -> Arc<SteckerDataChannel> {
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
        room_type: &DataRoomType,
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

    pub async fn listen_for_audio_channel(&self) -> anyhow::Result<SteckerAudioChannel> {
        let audio_channel = SteckerAudioChannel::create_channels();

        let audio_channel_tx = audio_channel.audio_channel_tx.clone();
        let close_tx = audio_channel.close.clone();

        self.peer_connection.on_track(Box::new(move |track, _, _| {
            println!("Received a track! :O");
            // receive and forward any content via a local track
            // this local track can then be passed to other connections
            let audio_channel_tx2 = audio_channel_tx.clone();
            let close_tx2 = close_tx.clone();

            tokio::spawn(async move {
                let local_track = Arc::new(TrackLocalStaticRTP::new(
                    track.codec().capability,
                    "audio".to_owned(),
                    "stecker".to_owned(),
                ));

                let _ = audio_channel_tx2.send(Some(local_track.clone()));

                // the actual forwarding
                // @todo improve error handling
                while let Ok((rtp, _)) = track.read_rtp().await {
                    let _ = local_track.write_rtp(&rtp).await;
                }

                println!("Nothing to transmit anymore?:O");
                let _ = close_tx2.send(());
            });
            Box::pin(async {})
        }));

        Ok(audio_channel)
    }

    pub async fn add_existing_audio_track(&self, track: Arc<TrackLocalStaticRTP>) -> () {
        let _ = self.peer_connection.add_track(track).await;
        // maybe add this one as well
        // https://github.com/webrtc-rs/webrtc/blob/62f2550799efe2dd36cdc950ad3f334b120c75bb/examples/examples/broadcast/broadcast.rs#L258-L265
    }

    pub async fn playback_audio_file(&self) -> anyhow::Result<()> {
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                ..Default::default()
            },
            "audio".to_owned(),
            "stecker".to_owned(),
        ));
        // the example adds the track differently? See
        // https://github.com/webrtc-rs/webrtc/blob/62f2550799efe2dd36cdc950ad3f334b120c75bb/examples/examples/play-from-disk-h264/play-from-disk-h264.rs#L234
        // let rtp_sender = self.peer_connection.add_track(audio_track.clone()).await?;
        let rtp_sender = self
            .peer_connection
            .add_track(Arc::clone(&audio_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        let audio_file_name = "/Users/scheiba/github/stecker/output.ogg".to_owned();
        tokio::spawn(async move {
            println!("Start something now!");
            // Open a IVF file and start reading using our IVFReader
            let file = File::open(audio_file_name)?;
            let reader = BufReader::new(file);
            // Open on oggfile in non-checksum mode.
            let (mut ogg, _) = OggReader::new(reader, true)?;

            // Wait for connection established
            // notify_audio.notified().await;

            let _ = sleep(Duration::from_secs(10)).await;

            // Read incoming RTCP packets
            // Before these packets are returned they are processed by interceptors. For things
            // like NACK this needs to be called.
            // the example calls this but it is not actually necessary?

            // tokio::spawn(async move {
            //     println!("Send some RTCP bullshit");
            //     let mut rtcp_buf = vec![0u8; 1500];
            //     while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            //     anyhow::Result::<()>::Ok(())
            // });

            println!("play audio from disk file output.ogg");

            // It is important to use a time.Ticker instead of time.Sleep because
            // * avoids accumulating skew, just calling time.Sleep didn't compensate for the time spent parsing the data
            // * works around latency issues with Sleep
            let mut ticker = tokio::time::interval(Duration::from_millis(20));

            // Keep track of last granule, the difference is the amount of samples in the buffer
            let mut last_granule: u64 = 0;
            while let Ok((page_data, page_header)) = ogg.parse_next_page() {
                // The amount of samples is the difference between the last and current timestamp
                let sample_count = page_header.granule_position - last_granule;
                last_granule = page_header.granule_position;
                let sample_duration = Duration::from_millis(sample_count * 1000 / 48000);

                audio_track
                    .write_sample(&Sample {
                        data: page_data.freeze(),
                        duration: sample_duration,
                        ..Default::default()
                    })
                    .await?;

                let _ = ticker.tick().await;
            }

            println!("Finished playback");

            anyhow::Result::<()>::Ok(())
        });

        println!("Hey");

        Ok(())
    }
}
