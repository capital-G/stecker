use std::str::FromStr;
use std::thread;

use tokio;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;

use shared::{
    api::APIClient,
    connections::SteckerWebRTCConnection,
    models::{RoomType, SteckerData},
};

pub struct Room {
    name: String,
    receiver: broadcast::Receiver<f32>,
    sender: broadcast::Sender<f32>,
    close_sender: broadcast::Sender<()>,
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
                    .create_data_channel(&RoomType::Float)
                    .await
                    .unwrap();
                let meta_data_channel = connection
                    .create_data_channel(&RoomType::Meta)
                    .await
                    .unwrap();
                let offer = connection.create_offer().await.unwrap();

                let api_client = APIClient { host: host2 };

                match api_client
                    .join_room(&name2, &shared::models::PublicRoomType::Float, &offer)
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
                    .create_data_channel(&RoomType::Float)
                    .await?;
                let offer = connection.create_offer().await?;

                tokio::spawn(async move {
                    let mut consume = true;
                    let mut webrtc_close_receiver = stecker_data_channel.close.clone().subscribe();

                    while consume {
                        tokio::select! {
                            msg_result = receiver.recv() =>{
                                match msg_result {
                                    Ok(msg) => {
                                        let _ = stecker_data_channel.outbound.send(SteckerData::F32(msg));
                                    },
                                    Err(err) => {
                                        println!("Got an error while pushing messages out: {err}");
                                        consume = false;
                                    },
                                }
                            },
                            _ = webrtc_close_receiver.recv() => {
                                println!("Received stop signal from webrtc on pushing values to WebRTC");
                                consume = false;
                            },
                        };
                    }
                    println!("Stopped forwarding messages from SC to WebRTC");
                });

                let api_client = APIClient { host: host2 };

                match api_client.create_room(&name2, &shared::models::PublicRoomType::Float, &offer).await {
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
