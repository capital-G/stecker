use std::str::FromStr;
use std::thread;
use std::time::Duration;

use opus::{Channels as OpusChannels, Encoder as OpusEncoder};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::wrap::caching::Caching;
use ringbuf::{HeapCons, HeapProd, HeapRb, SharedRb};
use shared::models::SteckerAPIRoomType;
use tokio::runtime::Runtime;
use tokio::sync::broadcast::{self, Receiver, Sender};

use shared::{
    api::APIClient,
    connections::SteckerWebRTCConnection,
    models::{DataRoomInternalType, SteckerData},
};
use webrtc::media::Sample;

pub struct Room {
    name: String,
    receiver: Receiver<f32>,
    sender: Sender<f32>,
    close_sender: Sender<()>,
    // we need to remember the last value pulled
    // from the queue
    last_value: f32,
}

impl Room {
    pub fn join_room(name: &str, host: &str) -> Self {
        let name2 = String::from_str(name).unwrap();
        let host2 = String::from_str(host).unwrap();

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

                let api_client = APIClient { host: host2 };

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

        let room = Room {
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
        let host2 = String::from_str(host).unwrap();

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

                let api_client = APIClient { host: host2 };

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

        let room = Room {
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
    // consumer: HeapCons<f32>,
    producer: HeapProd<f32>,
}

impl AudioRoomSender {
    // pub fn create_room(name: &str, host: &str) -> Self {
    //     // @todo ring buffer needs to have size of at least of ?
    //     let ring_buffer = HeapRb::<f32>::new(48000);

    //     let (producer, consumer) = ring_buffer.split();
    //     let (close_tx, _) = broadcast::channel::<()>(2);

    //     Self {
    //         name: name.to_owned(),
    //         close_sender: close_tx,
    //         producer,
    //         consumer,
    //     }
    // }

    pub fn create_room(name: &str, host: &str) -> Self {
        let name2 = String::from_str(name).unwrap();
        let host2 = String::from_str(host).unwrap();

        // @todo make this configurable?
        // 20 ms
        let frame_size: usize = 2400;
        // this needs to be
        let sample_rate: u32 = 48000;

        // @todo calculate the exapct size
        let ring_buffer = HeapRb::<f32>::new(48000);
        let (producer, consumer) = ring_buffer.split();

        let (sender, mut receiver) = broadcast::channel::<f32>(1024);
        let sender2 = sender.clone();

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
                    let opus_encoder = OpusEncoder::new(
                        sample_rate.into(),
                        OpusChannels::Mono,
                         opus::Application::Audio
                    ).expect("Could not init the opus encoder :O");

                    let mut audio_buffer = vec![0u8; frame_size];
                    // @todo this needs to be calculated based on the framerate (CONST) and frame size
                    let mut ticker = tokio::time::interval(Duration::from_millis(50));
                    loop {
                        let _ = ticker.tick().await;
                        if !consumer.is_empty() {
                            // what to do here?
                            let (foo, bar) = consumer.as_slices();
                            opus_encoder.encode_float(foo, &mut audio_buffer);
                            audio_track.write_sample(&Sample {
                                data: audio_buffer,
                                ..Default::default()
                            });
                        }
                    }
                });

                let api_client = APIClient { host: host2 };

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

        return AudioRoomSender {
            name: name.to_owned(),
            close_sender: close_sender,
            producer: producer,
        };
    }

    pub fn push_values_to_web(&mut self, values: Vec<f32>) {
        self.producer.push_iter(values.into_iter());
    }
}

fn create_room(name: &str, host: &str) -> Box<Room> {
    Box::new(Room::create_room(name, host))
}

fn join_room(name: &str, host: &str) -> Box<Room> {
    Box::new(Room::join_room(name, host))
}

fn recv_message(room: &mut Room) -> f32 {
    room.recv_message()
}

fn send_message(room: &mut Room, value: f32) -> f32 {
    room.send_message(value)
}

fn send_close_signal(room: &mut Room) {
    let _ = room.close_sender.send(());
}

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        type Room;
        fn create_room(name: &str, host: &str) -> Box<Room>;
        fn join_room(name: &str, host: &str) -> Box<Room>;
        fn recv_message(room: &mut Room) -> f32;
        fn send_message(room: &mut Room, value: f32) -> f32;
        fn send_close_signal(room: &mut Room);
    }
}
