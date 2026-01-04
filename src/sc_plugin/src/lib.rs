use std::f32::NAN;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use opus::{Channels as OpusChannels, Decoder as OpusDecoder, Encoder as OpusEncoder};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use shared::connections::ConnectionEvent;
use shared::models::{RoomFloatData, SteckerDataChanelTrait, SteckerDataChannel};
use tokio::runtime::Runtime;
use tokio::sync::broadcast::{self, Receiver, Sender};

use shared::{api::APIClient, connections::SteckerWebRTCConnection, models::SteckerData};
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tracing::{error, info, info_span, instrument, trace, Instrument, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{self, filter, fmt};

use webrtc::media::Sample;

fn setup_tracing() {
    let filter = filter::Targets::new()
        .with_default(Level::ERROR)
        .with_target("stecker_sc", Level::TRACE)
        .with_target("shared", Level::TRACE);

    // @todo impl FormatEvent to prefix logs with STECKER:
    let formatter = fmt::layer().with_ansi(false).compact().without_time();

    let subscriber = tracing_subscriber::registry().with(formatter).with(filter);

    let _ = subscriber.try_init();
}

pub struct DataRoomReceiver {
    close_sender: Arc<Notify>,
    value: watch::Receiver<f32>,
}

impl DataRoomReceiver {
    #[instrument()]
    pub fn join_room(name: String, host: String) -> Self {
        trace!("Join room");

        let (value_setter, value_getter) = watch::channel::<f32>(0.0f32);
        let close_sender = Arc::new(Notify::new());
        let close_receiver = close_sender.clone();

        thread::spawn(move || {
            {
            setup_tracing();
            let rt = Runtime::new().expect("Could not spawn async runtime");
            rt.block_on(async {
                let (events, _) = broadcast::channel::<ConnectionEvent>(16);
                let connection = SteckerWebRTCConnection::build_connection(events).await.expect("Could  not create peer connection");

                let data_channel = Arc::new(SteckerDataChannel::<RoomFloatData>::create_channels());
                let mut inbound_data = data_channel.inbound.subscribe();

                let _ = connection.create_data_channel(data_channel).await;
                let offer = connection.create_offer().await.unwrap();

                let api_client = APIClient::new(host.to_string());

                match api_client.join_room::<RoomFloatData>(&name, &offer).await {
                    Ok(answer) => {
                        connection.set_remote_description(answer).await.unwrap();
                        loop {
                            tokio::select! {
                                msg = inbound_data.recv() => {
                                    match msg {
                                        Ok(data) => {
                                            match value_setter.send(data) {
                                                Ok(_) => {},
                                                Err(err) => {
                                                    error!(?err, "Failed to send WebRTC message");
                                                    break;
                                                },
                                            }
                                        },
                                        Err(err) => {
                                            error!(?err, "Failed to receive webrtc data messages");
                                            break;
                                        },
                                    };
                                },
                                _ = connection.wait_for_disconnect() => {
                                    info!("Server closed connection!");
                                    break
                                }
                                _ = close_receiver.notified() => {
                                    trace!("Received supercollider close signal");
                                    break
                                }
                            }
                        }
                        connection.close().await.unwrap();
                        trace!("Close connection");
                    }
                    Err(err) => {
                        error!(?err, "Failed to join room");
                    }
                }
            });
        }.in_current_span()
        });
        Self {
            close_sender,
            value: value_getter,
        }
    }
    pub fn get_value(&self) -> f32 {
        *self.value.borrow()
    }
}

struct DataRoomSender {
    close_sender: Arc<Notify>,
    value: watch::Sender<f32>,
}

impl DataRoomSender {
    #[instrument]
    pub fn create_room(name: String, password: Option<String>, host: String) -> Self {
        let (value_setter, mut value_getter) = watch::channel::<f32>(0.0f32);
        let close_sender = Arc::new(Notify::new());
        let close_sender2 = close_sender.clone();
        let close_receiver = close_sender.clone();

        // @todo add this to a queue so that we don't spawn a thread in the RT thread...
        thread::spawn(move || {
            {
            setup_tracing();
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let (events, _) = broadcast::channel::<ConnectionEvent>(32);
                let connection = Arc::new(SteckerWebRTCConnection::build_connection(events).await.expect("Failed to create peer connection"));
                let data_channel = Arc::new(SteckerDataChannel::<RoomFloatData>::create_channels());
                let data_channel_outbound = data_channel.outbound.clone();
                println!("About to create data channel");
                connection.create_data_channel(data_channel).await.expect("Could not create data channel in peer connection");
                println!("Created data channel");
                let offer = connection.create_offer().await.expect("Could not create offer");

                let api_client = APIClient::new(host.to_string());

                match api_client.create_room::<RoomFloatData>(&name, password.as_ref().map(|x| x.as_str()), &offer).await {
                    Ok(answer) => {
                        let _ = connection.set_remote_description(answer.session_description).await.expect("Could not set session description");
                        trace!(password=answer.password, "Created data room on server");
                    }
                    Err(err) => {
                        error!(?err, "Failed to create room on server, closing connection");
                        close_sender.notify_one();
                    },
                }

                loop {
                    tokio::select! {
                        received = value_getter.changed() =>{
                            match received {
                                Ok(_) => {
                                    match data_channel_outbound.send(*value_getter.borrow_and_update()) {
                                        Ok(_) => {}
                                        Err(err) => {
                                            error!(?err, "Could not send out message");
                                        }
                                    }
                                },
                                Err(_) => {
                                    error!("Failed to receive value - terminating");
                                    break
                                },
                            }
                        },
                        // this doesn't seem to work - why?
                        // _ = connection.wait_for_disconnect() => {
                        //     error!("Server closed connection");
                        //     break
                        // },
                        _ = close_receiver.notified() => {
                            trace!("Stop consuming");
                            break
                        }
                    };
                }
                trace!("Stopped forwarding messages from SC to WebRTC");
                let _ = connection.close().await;
            })
        }.in_current_span()
        });

        Self {
            value: value_setter,
            close_sender: close_sender2,
        }
    }

    pub fn set_value(&self, value: f32) {
        let _ = self.value.send(value);
    }
}

pub struct AudioRoomSender {
    name: String,
    close_sender: Sender<()>,
    producer: HeapProd<f32>,
}

impl AudioRoomSender {
    #[instrument]
    pub fn create_room(name: &str, password: &str, host: &str) -> Self {
        setup_tracing();
        let name2 = name.to_owned();
        let host2 = host.to_owned();
        let password2 = password.to_owned();

        let span = info_span!("create_audio_room", room_name = name2);
        let span2 = span.clone();
        let span3 = span.clone();

        // @todo make this configurable?
        // 20 ms
        const FRAME_SIZE: usize = 960;
        // this needs to be
        let sample_rate: u32 = 48000;

        // @todo calculate the exapct size
        let ring_buffer = HeapRb::<f32>::new(48000);
        let (producer, mut consumer) = ring_buffer.split();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        todo!();

        /*

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let _guard = span.enter();
                let connection = SteckerWebRTCConnection::build_connection().await?;
                let audio_track = connection.create_audio_channel().await?;
                let meta_channel = connection.create_data_channel(&DataRoomInternalType::Meta).await?;
                let mut meta_recv = meta_channel.inbound.subscribe();
                let offer = connection.create_offer().await?;

                trace!(offer=offer, "Generated base64 encoded offer");

                // consume meta messages
                tokio::spawn(async move {
                    let _guard = span2.enter();
                    let mut webrtc_close_receiver = meta_channel.close.clone().subscribe();
                    loop {
                        tokio::select! {
                            meta_msg = meta_recv.recv() =>{
                                if let Ok(msg) = meta_msg {
                                    info!(message=?msg, "Received meta message");
                                }
                            },
                            _ = webrtc_close_receiver.recv() => {
                                trace!("Received stop signal from webrtc on pushing values to WebRTC");
                                break
                            },
                        };
                    }
                    info!("Stopped forwarding messages from SC to WebRTC");
                });

                // thread to push values to server
                tokio::spawn(async move {
                    let _guard = span3.enter();
                    let mut opus_encoder = OpusEncoder::new(
                        sample_rate.into(),
                        OpusChannels::Mono,
                        opus::Application::Audio
                    ).expect("Could not init the opus encoder :O");
                    let _ = opus_encoder.set_bitrate(opus::Bitrate::Bits(96000));
                    info!("Start encoding");
                    // let _ = opus_encoder.set_vbr(true);
                    // let mut opus_buffer = vec![0; FRAME_SIZE];
                    let mut raw_signal_buffer = [0.0f32; FRAME_SIZE];
                    // TODO: too large values here will crash (this is 512Byte)
                    let mut buf = [0; 4096];
                    // @todo this needs to be calculated based on the framerate (CONST) and frame size
                    let mut ticker = tokio::time::interval(Duration::from_millis(20));
                    loop {
                        let _ = ticker.tick().await;
                        if consumer.observe().occupied_len() >= FRAME_SIZE {
                            consumer.pop_slice(&mut raw_signal_buffer);
                            let encoding_result = opus_encoder.encode_float(&raw_signal_buffer, &mut buf);
                            match encoding_result {
                                Ok(packet_size) => {
                                    let result = audio_track.write_sample(&Sample {
                                        data: Bytes::copy_from_slice(&buf[0..packet_size]),
                                        duration: Duration::from_millis(20),
                                        ..Default::default()
                                    }).await;
                                    if let Err(err) = result {
                                        error!(error=?err, "Failed to write opus sample to the track");
                                    }
                                },
                                Err(err) => {
                                    error!(error=?err, "Failed to encode to opus frame.");
                                },
                            }
                        } else {
                            error!("Not enough values in ringbuf yet!");
                        }
                    }
                });

                let api_client = APIClient::new(host2.to_string());

                match api_client.create_room(
                        &name2,
                        Some(&password2),
                        &shared::models::SteckerAPIRoomType::Audio,
                        &offer,
                    ).await {
                    Ok(answer) => {
                        let _ = connection.set_remote_description(answer.session_description).await.expect("Could not set remote description!");

                        // // @todo wait for actual stop signal here
                        let _ = sc_close_receiver.recv().await;

                        Ok(())
                    }
                    Err(err) => {
                        error!(error=?err, "Failed to create audio room on server.");
                        Err(err)
                    },
                }
            })
        });

        trace!("Created the audio sender");

        return AudioRoomSender {
            name: name.to_owned(),
            close_sender: close_sender,
            producer: producer,
        };
         */
    }

    pub fn push_values_to_web(&mut self, values: &[f32]) {
        self.producer.push_iter(values.into_iter().cloned());
    }
}

pub struct AudioRoomReceiver {
    name: String,
    close_sender: Sender<()>,
    consumer: HeapCons<f32>,
}

impl AudioRoomReceiver {
    pub fn create_room(name: &str, host: &str, buffer_length: i32) -> Self {
        setup_tracing();
        // @todo we are assuming 48khz
        let name2 = name.to_owned();
        let name3 = name.to_owned();
        let host2 = host.to_owned();

        let span = info_span!("join_audio_room", room_name = name2);
        let span2 = span.clone();
        let span3 = span.clone();

        // @todo calculate the minimum needed size
        let ring_buffer = HeapRb::<f32>::new(24000);
        let (mut producer, consumer) = ring_buffer.split();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        todo!();

        /*

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let _guard = span.enter();
                let connection = SteckerWebRTCConnection::build_connection().await?;
                // let audio_track = connection.listen_for_audio_channel().await?;
                let meta_channel = connection.create_data_channel(&DataRoomInternalType::Meta).await?;
                let mut meta_recv = meta_channel.inbound.subscribe();
                let mut audio_track_receiver = connection.listen_for_remote_audio_track().await;
                let offer = connection.create_offer().await?;

                trace!(offer=offer, "Generated base64 offer.");

                // consume meta messages
                tokio::spawn(async move {
                    let _guard = span2.enter();
                    let mut webrtc_close_receiver = meta_channel.close.clone().subscribe();
                    loop {
                        tokio::select! {
                            meta_msg = meta_recv.recv() =>{
                                if let Ok(msg) = meta_msg {
                                    info!(message=?msg, "Received meta message");
                                }
                            },
                            _ = webrtc_close_receiver.recv() => {
                                trace!("Received stop signal from webrtc on pushing values to WebRTC");
                                break
                            },
                        };
                    }
                    info!("Stopped forwarding messages from SC to WebRTC");
                });

                tokio::spawn(async move {
                    let _guard = span3.enter();
                    let mut opus_decoder = OpusDecoder::new(48000, OpusChannels::Mono).expect("Could not init the opus decoder");

                    // max size from https://opus-codec.org/docs/opus_api-1.2/group__opus__decoder.html#ga9c554b8c0214e24733a299fe53bb3bd2
                    let mut raw_signal_buffer: Vec<f32> = vec![0.0; 5760];
                    trace!("Wait for audio track to be received");

                        let received_audio_track = audio_track_receiver.recv().await.clone().unwrap();

                        info!("Found an track! Start decoding");

                        while let Ok((rtp, _)) = received_audio_track.read_rtp().await {
                            match opus_decoder.decode_float(&*rtp.payload, &mut raw_signal_buffer, false) {
                                Ok(opus_samples) => {
                                    // Push the number of opus_samples from my signal buffer into the ring buffer
                                    producer.push_slice(&raw_signal_buffer[..opus_samples]);
                                },
                                Err(err) => {
                                    error!(error=?err, "Error decoding opus frame");
                                },
                            }
                        }
                });

                let api_client = APIClient::new(host2);

                match api_client.join_room(&name2, &shared::models::SteckerAPIRoomType::Audio, &offer).await {
                    Ok(answer) => {
                        trace!("Received remote offer");
                        let _ = connection.set_remote_description(answer).await.expect("Could not set remote description!");

                        // // @todo wait for actual stop signal here
                        let _ = sc_close_receiver.recv().await;

                        info!("Close connection now");
                        Ok(())
                    }
                    Err(err) => {
                        error!(error=?err, "Failed to create audio room on server");
                        Err(err)
                    },
                }
            })
        });

        Self {
            name: name3,
            close_sender,
            consumer,
        }
        */
    }

    pub fn pull_values_from_web(&mut self, values: &mut [f32]) -> () {
        if self.consumer.occupied_len() < values.len() {
            // println!("Not enough values in decodec ringbuf");
            for v in values.iter_mut() {
                *v = 0.0;
            }
        } else {
            self.consumer.pop_slice(values);
        }
    }
}

// data sender
unsafe fn create_data_room(name: &str, password: &str, host: &str) -> *mut DataRoomSender {
    // @todo to_string allocates on the RT thread!
    Box::into_raw(Box::new(DataRoomSender::create_room(
        name.to_string(),
        Some(password.to_string()),
        host.to_string(),
    )))
}

unsafe fn send_data_message(data_room: *mut DataRoomSender, value: f32) {
    unsafe { (*data_room).set_value(value) }
}

unsafe fn close_data_sender_room(room: *mut DataRoomSender) {
    if (!room.is_null()) {
        let room = unsafe { Box::from_raw(room) };
        room.close_sender.notify_one();
        // @todo defer this to a delete queue which gets consumed in its own thread
        drop(room);
    }
}

// data receiver
unsafe fn join_data_room(name: &str, host: &str) -> *mut DataRoomReceiver {
    // @todo this allocates on the RT thread!
    Box::into_raw(Box::new(DataRoomReceiver::join_room(
        name.to_string(),
        host.to_string(),
    )))
}

unsafe fn recv_data_message(data_room: *mut DataRoomReceiver) -> f32 {
    (*data_room).get_value()
}

unsafe fn close_data_receiver_room(data_room: *mut DataRoomReceiver) {
    (*data_room).close_sender.notify_one();
}

fn create_audio_room_sender(name: &str, password: &str, host: &str) -> Box<AudioRoomSender> {
    Box::new(AudioRoomSender::create_room(name, password, host))
}

unsafe fn push_values_to_web(audio_room: &mut AudioRoomSender, values: *mut f32, num_samples: i32) {
    let slice = unsafe { std::slice::from_raw_parts_mut(values, num_samples.try_into().unwrap()) };
    // println!("Got some values? {} {}", slice[], num_samples);
    let _ = audio_room.push_values_to_web(slice);
}

fn create_audio_room_receiver(
    name: &str,
    host: &str,
    buffer_length: i32,
) -> Box<AudioRoomReceiver> {
    Box::new(AudioRoomReceiver::create_room(name, host, buffer_length))
}

unsafe fn pull_values_from_web(
    audio_room: &mut AudioRoomReceiver,
    values: *mut f32,
    num_samples: i32,
) {
    let slice = unsafe { std::slice::from_raw_parts_mut(values, num_samples.try_into().unwrap()) };
    audio_room.pull_values_from_web(slice);
}

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        type DataRoomSender;
        unsafe fn create_data_room(name: &str, password: &str, host: &str) -> *mut DataRoomSender;
        unsafe fn send_data_message(room: *mut DataRoomSender, value: f32);
        unsafe fn close_data_sender_room(room: *mut DataRoomSender);

        type DataRoomReceiver;
        unsafe fn join_data_room(name: &str, host: &str) -> *mut DataRoomReceiver;
        unsafe fn recv_data_message(room: &mut DataRoomReceiver) -> f32;
        unsafe fn close_data_receiver_room(room: *mut DataRoomReceiver);

        type AudioRoomSender;
        fn create_audio_room_sender(name: &str, password: &str, host: &str)
            -> Box<AudioRoomSender>;
        unsafe fn push_values_to_web(
            audio_room: &mut AudioRoomSender,
            values: *mut f32,
            num_samples: i32,
        );

        type AudioRoomReceiver;
        fn create_audio_room_receiver(
            name: &str,
            host: &str,
            buffer_length: i32,
        ) -> Box<AudioRoomReceiver>;
        unsafe fn pull_values_from_web(
            audio_room: &mut AudioRoomReceiver,
            values: *mut f32,
            num_samples: i32,
        );
    }
}
