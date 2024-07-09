use std::str::FromStr;
use std::thread;

use tokio;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;

use shared::{api::APIClient, connections::SteckerWebRTCConnection};

pub struct Room {
    name: String,
    receiver: broadcast::Receiver<f32>,
    sender: broadcast::Sender<f32>,
    // we need to remember the last value pulled
    // from the queue
    last_value: f32,
}

const HOST: &str = "http://127.0.0.1:8000";
const DATA_CHANNEL_NAME: &str = "foo";

impl Room {
    pub fn join_room(name: &str) -> Self {
        let name2 = String::from_str(name).unwrap();

        let (sender, _) = broadcast::channel::<f32>(1024);
        let sender2 = sender.clone();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await.unwrap();
                let stecker_data_channel = connection.create_data_channel::<f32>(DATA_CHANNEL_NAME).await.unwrap();
                let offer = connection.create_offer().await.unwrap();

                let api_client = APIClient {
                    host: HOST.to_string(),
                };

                match api_client.join_room(&name2, &offer).await {
                    Ok(answer) => {
                        connection.set_remote_description(answer).await.unwrap();

                        let mut inbound_receiver = stecker_data_channel.inbound.clone().subscribe();
                        let mut close_receiver = stecker_data_channel.close_trigger.clone().subscribe();
                        let mut consume = true;

                        while consume {
                            tokio::select! {
                                msg = inbound_receiver.recv() => {
                                    match msg {
                                        Ok(m) => {
                                            match sender.send(m) {
                                                Ok(_) => {}
                                                Err(err) => {
                                                    println!("Error while forwarding WebRTC message to SC: {err}");
                                                    consume = false;
                                                }
                                            }
                                        }
                                        Err(err) => println!("Error while receiving WebRTC message: {err}"),
                                    }
                                },
                                _ = close_receiver.recv() => {
                                    println!("received close signal!");
                                    consume = false;
                                }
                            }
                        }
                        println!("Close connection now");
                        connection.close().await.unwrap();
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
        };

        room
    }

    pub fn create_room(name: &str) -> Self {
        let name2 = String::from_str(name).unwrap();

        let (sender, mut receiver) = broadcast::channel::<f32>(1024);
        let sender2 = sender.clone();
        let receiver2 = sender2.clone().subscribe();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await?;
                let stecker_data_channel = connection
                    .create_data_channel::<f32>(DATA_CHANNEL_NAME)
                    .await?;
                let offer = connection.create_offer().await?;

                tokio::spawn(async move {
                    let mut consume = true;
                    let mut close_receiver = stecker_data_channel.close_trigger.clone().subscribe();

                    while consume {
                        tokio::select! {
                            msg_result = receiver.recv() =>{
                                match msg_result {
                                    Ok(msg) => {
                                        let _ = stecker_data_channel.outbound.send(msg);
                                    },
                                    Err(err) => {
                                        println!("Got an error while pushing messages out: {err}");
                                        consume = false;
                                    },
                                }
                            },
                            _ = close_receiver.recv() => {
                                println!("Received stop signal on pushing values to WebRTC");
                                consume = false;
                            }
                        };
                    }

                    println!("Stopped forwarding messages from SC to WebRTC");
                });

                let api_client = APIClient {
                    host: HOST.to_string(),
                };

                match api_client.create_room(&name2, &offer).await {
                    Ok(answer) => {
                        connection.set_remote_description(answer).await?;

                        // @todo wait for actual stop signal here
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                println!("Received");
                            }
                        };

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

fn create_room(name: &str) -> Box<Room> {
    Box::new(Room::create_room(name))
}

fn join_room(name: &str) -> Box<Room> {
    Box::new(Room::join_room(name))
}

fn recv_message(room: &mut Room) -> f32 {
    room.recv_message()
}

fn send_message(room: &mut Room, value: f32) -> f32 {
    room.send_message(value)
}

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        type Room;
        fn create_room(name: &str) -> Box<Room>;
        fn join_room(name: &str) -> Box<Room>;
        fn recv_message(room: &mut Room) -> f32;
        fn send_message(room: &mut Room, value: f32) -> f32;
    }
}
