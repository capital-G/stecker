use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use opus::{Channels as OpusChannels, Decoder as OpusDecoder, Encoder as OpusEncoder};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use shared::connections::ConnectionEvent;
use shared::models::{RoomAudioData, RoomFloatData, SteckerDataChanelTrait, SteckerDataChannel};
use tokio::runtime::Runtime;
use tokio::sync::broadcast::{self, Sender};

use shared::{api::APIClient, connections::SteckerWebRTCConnection};
use tokio::sync::{watch, Notify};
use tracing::{error, info, info_span, instrument, trace, Level};
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
            setup_tracing();
            let run = || -> anyhow::Result<()> {
                let rt = Runtime::new()?;
                rt.block_on(async {
                    let (events, _) = broadcast::channel::<ConnectionEvent>(16);
                    let connection = SteckerWebRTCConnection::build_connection(events).await?;

                    let data_channel =
                        Arc::new(SteckerDataChannel::<RoomFloatData>::create_channels());
                    let mut inbound_data = data_channel.inbound.subscribe();

                    connection.create_data_channel(data_channel).await?;
                    let offer = connection.create_offer().await?;

                    let api_client = APIClient::new(host.to_string());
                    let answer = api_client.join_room::<RoomFloatData>(&name, &offer).await?;
                    connection.set_remote_description(answer).await?;

                    loop {
                        tokio::select! {
                            msg = inbound_data.recv() => {
                                match msg {
                                    Ok(data) => {
                                        if value_setter.send(data).is_err() {
                                            break;
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
                    let _ = connection.close().await;
                    Ok(())
                })
            };
            if let Err(err) = run() {
                error!(error=?err, "Data receiver failed");
            }
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

        thread::spawn(move || {
            setup_tracing();
            let result: anyhow::Result<()> = (|| {
                let rt = Runtime::new()?;
                rt.block_on(async {
                    let (events, _) = broadcast::channel::<ConnectionEvent>(32);
                    let connection = Arc::new(SteckerWebRTCConnection::build_connection(events).await?);

                    let data_channel = Arc::new(SteckerDataChannel::<RoomFloatData>::create_channels());
                    let data_channel_outbound = data_channel.outbound.clone();
                    connection.create_data_channel(data_channel).await?;
                    let offer = connection.create_offer().await?;

                    let api_client = APIClient::new(host.to_string());

                    let answer = match api_client.create_room::<RoomFloatData>(&name, password.as_ref().map(|x| x.as_str()), &offer).await {
                        Ok(answer) => answer,
                        Err(err) => {
                            close_sender.notify_one();
                            return Err(err);
                        },
                    };
                    connection.set_remote_description(answer.session_description).await?;
                    trace!(password=answer.password, "Created data room on server");

                    loop {
                        tokio::select! {
                            received = value_getter.changed() => {
                                match received {
                                    Ok(_) => {
                                        let _ = data_channel_outbound.send(*value_getter.borrow_and_update());
                                    },
                                    Err(_) => {
                                        error!("Failed to receive value - terminating");
                                        break
                                    },
                                }
                            },
                            _ = close_receiver.notified() => {
                                trace!("Stop consuming");
                                break
                            }
                        };
                    }
                    let _ = connection.close().await;
                    Ok(())
                })
            })();
            if let Err(err) = result {
                error!(error=?err, "Data sender failed");
            }
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
        let encoding_span = span.clone();

        const FRAME_SIZE: usize = 960;
        let sample_rate: u32 = 48000;

        let ring_buffer = HeapRb::<f32>::new(48000);
        let (producer, mut consumer) = ring_buffer.split();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        thread::spawn(move || {
            setup_tracing();
            let run = || -> anyhow::Result<()> {
                let rt = Runtime::new()?;
                rt.block_on(async {
                    let (events, _) = broadcast::channel::<ConnectionEvent>(16);
                    let connection = SteckerWebRTCConnection::build_connection(events).await?;
                    let audio_track = connection.create_audio_channel().await?;
                    let offer = connection.create_offer().await?;

                    trace!(offer=offer, "Generated base64 encoded offer");

                    tokio::spawn(async move {
                        let _guard = encoding_span.enter();
                        let mut opus_encoder = OpusEncoder::new(
                            sample_rate.into(),
                            OpusChannels::Mono,
                            opus::Application::Audio
                        )?;
                        let _ = opus_encoder.set_bitrate(opus::Bitrate::Bits(96000));
                        info!("Start encoding");
                        let mut raw_signal_buffer = [0.0f32; FRAME_SIZE];
                        let mut buf = [0; 4096];
                        let mut ticker = tokio::time::interval(Duration::from_millis(20));
                        loop {
                            let _ = ticker.tick().await;
                            if consumer.observe().occupied_len() >= FRAME_SIZE {
                                consumer.pop_slice(&mut raw_signal_buffer);
                                match opus_encoder.encode_float(&raw_signal_buffer, &mut buf) {
                                    Ok(packet_size) => {
                                        if let Err(err) = audio_track.write_sample(&Sample {
                                            data: Bytes::copy_from_slice(&buf[0..packet_size]),
                                            duration: Duration::from_millis(20),
                                            ..Default::default()
                                        }).await {
                                            error!(error=?err, "Failed to write opus sample to the track");
                                        }
                                    },
                                    Err(err) => {
                                        error!(error=?err, "Failed to encode to opus frame");
                                    },
                                }
                            } else {
                                trace!("Not enough values in ringbuf yet");
                            }
                        }
                        #[allow(unreachable_code)]
                        Ok::<(), anyhow::Error>(())
                    });

                    let api_client = APIClient::new(host2.to_string());
                    let answer = api_client.create_room::<RoomAudioData>(&name2, Some(&password2), &offer).await?;
                    connection.set_remote_description(answer.session_description).await?;
                    let _ = sc_close_receiver.recv().await;
                    Ok(())
                })
            };
            if let Err(err) = run() {
                error!(error=?err, "Audio sender failed");
            }
        });

        trace!("Created the audio sender");

        AudioRoomSender {
            name: name.to_owned(),
            close_sender,
            producer,
        }
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
        let name2 = name.to_owned();
        let host2 = host.to_owned();

        let span = info_span!("join_audio_room", room_name = name2);
        let decoding_span = span.clone();

        let ring_buffer = HeapRb::<f32>::new(24000);
        let (mut producer, consumer) = ring_buffer.split();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        thread::spawn(move || {
            setup_tracing();
            let run = || -> anyhow::Result<()> {
                let rt = Runtime::new()?;
                rt.block_on(async {
                    let (events, _) = broadcast::channel::<ConnectionEvent>(16);
                    let connection = SteckerWebRTCConnection::build_connection(events).await?;
                    let mut audio_events = connection.connection_events.subscribe();
                    connection.add_recvonly_audio_transceiver().await?;
                    let offer = connection.create_offer().await?;

                    trace!(offer = offer, "Generated base64 offer");

                    let api_client = APIClient::new(host2);
                    let answer = api_client
                        .join_room::<RoomAudioData>(&name2, &offer)
                        .await?;
                    connection.set_remote_description(answer).await?;

                    tokio::spawn(async move {
                        let _guard = decoding_span.enter();
                        let mut opus_decoder = OpusDecoder::new(48000, OpusChannels::Mono)?;
                        let mut raw_signal_buffer: Vec<f32> = vec![0.0; 5760];
                        trace!("Wait for audio track to be received");

                        let received_audio_track = loop {
                            if let Ok(ConnectionEvent::NewAudioChannel(track)) =
                                audio_events.recv().await
                            {
                                break track;
                            }
                        };

                        info!("Found a track! Start decoding");

                        while let Ok((rtp, _)) = received_audio_track.read_rtp().await {
                            match opus_decoder.decode_float(
                                &*rtp.payload,
                                &mut raw_signal_buffer,
                                false,
                            ) {
                                Ok(opus_samples) => {
                                    producer.push_slice(&raw_signal_buffer[..opus_samples]);
                                }
                                Err(err) => {
                                    error!(error=?err, "Error decoding opus frame");
                                }
                            }
                        }
                        Ok::<(), anyhow::Error>(())
                    });

                    let _ = sc_close_receiver.recv().await;
                    Ok(())
                })
            };
            if let Err(err) = run() {
                error!(error=?err, "Audio receiver failed");
            }
        });

        Self {
            name: name.to_owned(),
            close_sender,
            consumer,
        }
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
