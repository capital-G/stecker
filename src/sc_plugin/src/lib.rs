use std::str::FromStr;
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use opus::{Channels as OpusChannels, Decoder as OpusDecoder, Encoder as OpusEncoder};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use shared::models::SteckerAPIRoomType;
use tokio::runtime::Runtime;
use tokio::sync::broadcast::{self, Receiver, Sender};

use shared::{
    api::APIClient,
    connections::SteckerWebRTCConnection,
    models::{DataRoomInternalType, SteckerData},
};
use webrtc::media::Sample;

pub struct DataRoom {
    name: String,
    receiver: Receiver<f32>,
    sender: Sender<f32>,
    close_sender: Sender<()>,
    // we need to remember the last value pulled
    // from the queue
    last_value: f32,
}

impl DataRoom {
    pub fn join_room(name: &str, host: &str) -> Self {
        let name2 = String::from_str(name).unwrap();
        let host2 = host.to_owned();

        let (sender, _) = broadcast::channel::<f32>(1024);
        let sender2 = sender.clone();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await.unwrap();
                let stecker_data_channel = connection
                    .create_data_channel(&DataRoomInternalType::Float)
                    .await
                    .unwrap();
                let meta_data_channel = connection
                    .create_data_channel(&DataRoomInternalType::Meta)
                    .await
                    .unwrap();
                let offer = connection.create_offer().await.unwrap();

                let api_client = APIClient::new(host2);

                match api_client
                    .join_room(
                        &name2,
                        &SteckerAPIRoomType::Data(shared::models::DataRoomPublicType::Float),
                        &offer,
                    )
                    .await
                {
                    Ok(answer) => {
                        connection.set_remote_description(answer).await.unwrap();

                        let mut inbound_receiver = stecker_data_channel.inbound.clone().subscribe();
                        let mut meta_receiver = meta_data_channel.inbound.clone().subscribe();
                        let mut webrtc_close_receiver =
                            stecker_data_channel.close.clone().subscribe();

                        loop {
                            tokio::select! {
                                msg = inbound_receiver.recv() => {
                                    if let Ok(SteckerData::F32(m)) = msg {
                                        let _ = sender.send(m);
                                    } else {
                                        println!("Error on forwarding message to webrtc");
                                    }
                                },
                                meta_msg = meta_receiver.recv() => {
                                    if let Ok(SteckerData::String(m)) = meta_msg {
                                        println!("META: {m}");
                                    } else {
                                        println!("Error on receiving meta message")
                                    }
                                }
                                _ = webrtc_close_receiver.recv() => {
                                    println!("received webrtc close signal!");
                                    break
                                }
                                _ = sc_close_receiver.recv() => {
                                    println!("Received supercollider close signal");
                                    break
                                }
                            }
                        }
                        connection.close().await.unwrap();
                        println!("Close connection now");
                    }
                    Err(err) => {
                        println!("Some error joining room {err}");
                    }
                }
            });
        });

        let room = Self {
            name: name.to_string(),
            receiver: sender2.subscribe(),
            sender: sender2,
            last_value: -1.0,
            close_sender,
        };

        room
    }

    pub fn create_room(name: &str, host: &str) -> Self {
        let name2 = String::from_str(name).unwrap();
        let host2 = host.to_owned();

        let (sender, mut receiver) = broadcast::channel::<f32>(1024);
        let sender2 = sender.clone();
        let receiver2 = sender2.clone().subscribe();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await?;
                let stecker_data_channel = connection
                    .create_data_channel(&DataRoomInternalType::Float)
                    .await?;
                let offer = connection.create_offer().await?;

                tokio::spawn(async move {
                    let mut webrtc_close_receiver = stecker_data_channel.close.clone().subscribe();
                    loop {
                        tokio::select! {
                            msg_result = receiver.recv() =>{
                                match msg_result {
                                    Ok(msg) => {
                                        let _ = stecker_data_channel.outbound.send(SteckerData::F32(msg));
                                    },
                                    Err(err) => {
                                        println!("Got an error while pushing messages out: {err}");
                                        break
                                    },
                                }
                            },
                            _ = webrtc_close_receiver.recv() => {
                                println!("Received stop signal from webrtc on pushing values to WebRTC");
                                break
                            },
                        };
                    }
                    println!("Stopped forwarding messages from SC to WebRTC");
                });

                let api_client = APIClient::new(host2);

                match api_client.create_room(&name2, &shared::models::SteckerAPIRoomType::Data(shared::models::DataRoomPublicType::Float), &offer).await {
                    Ok(answer) => {
                        connection.set_remote_description(answer).await?;

                        // @todo wait for actual stop signal here
                        let _ = sc_close_receiver.recv().await;

                        println!("Close connection now");
                        connection.close().await?;
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            })
        });

        let room = Self {
            name: name.to_string(),
            receiver: receiver2,
            sender: sender2,
            last_value: -1.0,
            close_sender,
        };

        room
    }

    pub fn recv_message(&mut self) -> f32 {
        if !self.receiver.is_empty() {
            // consume old messages from the queue
            // so that there is only one message left
            while self.receiver.len() > 1 {
                let _ = self.receiver.try_recv();
            }

            match self.receiver.try_recv() {
                Ok(msg) => {
                    self.last_value = msg;
                }
                Err(err) => {
                    println!("Got an error while receiving values: {err}");
                }
            }
        };
        self.last_value
    }

    pub fn send_message(&mut self, value: f32) -> f32 {
        let _ = self.sender.send(value);
        1.0
    }
}

pub struct AudioRoomSender {
    name: String,
    close_sender: Sender<()>,
    producer: HeapProd<f32>,
}

impl AudioRoomSender {
    pub fn create_room(name: &str, host: &str) -> Self {
        let name2 = name.to_owned();
        let host2 = host.to_owned();

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

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await?;
                let audio_track = connection.create_audio_channel().await?;
                let meta_channel = connection.create_data_channel(&DataRoomInternalType::Meta).await?;
                let mut meta_recv = meta_channel.inbound.subscribe();
                let offer = connection.create_offer().await?;

                println!("Our base64 offer is: {offer}");

                // consume meta messages
                tokio::spawn(async move {
                    let mut webrtc_close_receiver = meta_channel.close.clone().subscribe();
                    loop {
                        tokio::select! {
                            meta_msg = meta_recv.recv() =>{
                                if let Ok(msg) = meta_msg {
                                    println!("META: {msg}");
                                }
                            },
                            _ = webrtc_close_receiver.recv() => {
                                println!("Received stop signal from webrtc on pushing values to WebRTC");
                                break
                            },
                        };
                    }
                    println!("Stopped forwarding messages from SC to WebRTC");
                });

                // thread to push values to server
                tokio::spawn(async move {
                    let mut opus_encoder = OpusEncoder::new(
                        sample_rate.into(),
                        OpusChannels::Mono,
                        opus::Application::Audio
                    ).expect("Could not init the opus encoder :O");
                    let _ = opus_encoder.set_bitrate(opus::Bitrate::Bits(96000));
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
                                        println!("Failed to write opus sample to the track: {err}");
                                    }
                                },
                                Err(err) => {
                                    println!("Failed to encode to opus: {err}");
                                },
                            }
                        } else {
                            println!("Not enough values in ringbuf yet :O");
                        }
                    }
                });

                let api_client = APIClient::new(host2.to_string());

                match api_client.create_room(&name2, &shared::models::SteckerAPIRoomType::Audio, &offer).await {
                    Ok(answer) => {
                        let _ = connection.set_remote_description(answer).await.expect("Could not set remote description!");

                        // // @todo wait for actual stop signal here
                        let _ = sc_close_receiver.recv().await;

                        // println!("Close connection now");
                        // connection.close().await?;
                        Ok(())
                    }
                    Err(err) => {
                        println!("Failed to create audio room on server: {err}");
                        Err(err)
                    },
                }
            })
        });

        println!("Created the audio sender :O");

        return AudioRoomSender {
            name: name.to_owned(),
            close_sender: close_sender,
            producer: producer,
        };
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
        // @todo we are assuming 48khz
        let name2 = name.to_owned();
        let name3 = name.to_owned();
        let host2 = host.to_owned();

        // @todo calculate the minimum needed size
        let ring_buffer = HeapRb::<f32>::new(24000);
        let (mut producer, consumer) = ring_buffer.split();

        let (close_sender, _) = broadcast::channel::<()>(1);
        let mut sc_close_receiver = close_sender.subscribe();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await?;
                // let audio_track = connection.listen_for_audio_channel().await?;
                let meta_channel = connection.create_data_channel(&DataRoomInternalType::Meta).await?;
                let mut meta_recv = meta_channel.inbound.subscribe();
                let mut audio_track_receiver = connection.listen_for_remote_audio_track().await;
                let offer = connection.create_offer().await?;

                println!("Our base64 offer is: {offer}");

                // consume meta messages
                tokio::spawn(async move {
                    let mut webrtc_close_receiver = meta_channel.close.clone().subscribe();
                    loop {
                        tokio::select! {
                            meta_msg = meta_recv.recv() =>{
                                if let Ok(msg) = meta_msg {
                                    println!("META: {msg}");
                                }
                            },
                            _ = webrtc_close_receiver.recv() => {
                                println!("Received stop signal from webrtc on pushing values to WebRTC");
                                break
                            },
                        };
                    }
                    println!("Stopped forwarding messages from SC to WebRTC");
                });

                tokio::spawn(async move {
                    let mut opus_decoder = OpusDecoder::new(48000, OpusChannels::Mono).expect("Could not init the opus decoder");

                    // max size from https://opus-codec.org/docs/opus_api-1.2/group__opus__decoder.html#ga9c554b8c0214e24733a299fe53bb3bd2
                    let mut raw_signal_buffer: Vec<f32> = vec![0.0; 5760];
                    println!("Wait for audio track to be received");

                        let received_audio_track = audio_track_receiver.recv().await.clone().unwrap();

                        println!("Found some track! Start decoding");

                        while let Ok((rtp, _)) = received_audio_track.read_rtp().await {
                            match opus_decoder.decode_float(&*rtp.payload, &mut raw_signal_buffer, false) {
                                Ok(opus_samples) => {
                                    // Push the number of opus_samples from my signal buffer into the ring buffer
                                    producer.push_slice(&raw_signal_buffer[..opus_samples]);
                                },
                                Err(err) => {
                                    println!("Error decoding opus frame: {:?}", err);
                                },
                            }
                        }
                });

                let api_client = APIClient::new(host2);

                match api_client.join_room(&name2, &shared::models::SteckerAPIRoomType::Audio, &offer).await {
                    Ok(answer) => {
                        println!("Received remote offer");
                        let _ = connection.set_remote_description(answer).await.expect("Could not set remote description!");

                        // // @todo wait for actual stop signal here
                        let _ = sc_close_receiver.recv().await;

                        println!("Close connection now");
                        Ok(())
                    }
                    Err(err) => {
                        println!("Failed to create audio room on server: {err}");
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

fn create_data_room(name: &str, host: &str) -> Box<DataRoom> {
    Box::new(DataRoom::create_room(name, host))
}

fn join_data_room(name: &str, host: &str) -> Box<DataRoom> {
    Box::new(DataRoom::join_room(name, host))
}

fn recv_data_message(data_room: &mut DataRoom) -> f32 {
    data_room.recv_message()
}

fn send_data_message(data_room: &mut DataRoom, value: f32) -> f32 {
    data_room.send_message(value)
}

fn send_data_close_signal(data_room: &mut DataRoom) {
    let _ = data_room.close_sender.send(());
}

fn create_audio_room_sender(name: &str, host: &str) -> Box<AudioRoomSender> {
    Box::new(AudioRoomSender::create_room(name, host))
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
        type DataRoom;
        fn create_data_room(name: &str, host: &str) -> Box<DataRoom>;
        fn join_data_room(name: &str, host: &str) -> Box<DataRoom>;
        fn recv_data_message(room: &mut DataRoom) -> f32;
        fn send_data_message(room: &mut DataRoom, value: f32) -> f32;
        fn send_data_close_signal(room: &mut DataRoom);

        type AudioRoomSender;
        fn create_audio_room_sender(name: &str, host: &str) -> Box<AudioRoomSender>;
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
