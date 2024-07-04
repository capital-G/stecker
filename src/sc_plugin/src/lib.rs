use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tokio;
use tokio::runtime::Runtime;
use tokio::time::sleep;

use shared::{
    api::APIClient,
    connections::SteckerWebRTCConnection
};

pub struct Room {
    name: String,
    next_float: Arc<Mutex<f32>>,
}

const HOST: &str = "http://127.0.0.1:8000";

impl Room {
    pub fn new(name: &str) -> Self {
        let next_float = Arc::new(Mutex::new(0.0));
        let room = Room {
            name: name.to_string(),
            next_float: next_float.clone()
        };

        let next_float2 = next_float.clone();
        let name2 = String::from_str(name).unwrap();

        let t = thread::spawn(move || {

            let rt  = Runtime::new().unwrap();
            rt.block_on(async {
                let connection = SteckerWebRTCConnection::build_connection().await.unwrap();
                let stecker_data_channel = connection.create_data_channel::<String>("foo").await.unwrap();
                let offer = connection.create_offer().await.unwrap();

                let api_client = APIClient {
                    host: HOST.to_string(),
                };

                match api_client.join_room(&name2, &offer).await {
                    Ok(answer) => {
                        // Apply the answer as the remote description
                        connection.set_remote_description(answer).await.unwrap();

                        let mut receiver = stecker_data_channel.inbound.clone().subscribe();

                        tokio::spawn(async move {
                            loop {
                                let msg = receiver.recv().await;
                                match msg {
                                    Ok(m) => println!("received a message: {m}"),
                                    Err(_) => println!("Error while receiving message"),
                                }
                            }
                        });

                        println!("Press ctrl-c to stop");
                        let mut close_receiver = stecker_data_channel.close_trigger.clone().subscribe();
                        tokio::select! {
                            _ = close_receiver.recv() => {
                                println!("received close signal!");
                            }
                            _ = tokio::signal::ctrl_c() => {
                                println!();
                            }
                        };

                        connection.close().await.unwrap();

                    }
                    Err(err) => {
                        println!("Some error joining room {err}");
                    },
                }


                loop {
                    sleep(Duration::from_millis(1000)).await;
                    println!("Hello from {name2}");
                    let mut x = next_float2.lock().unwrap();
                    *x = *x+0.01;
                }
            });
        });

        room
    }

    pub fn recv_message(&self) -> f32 {
        *self.next_float.lock().unwrap()
    }
}

fn create_room(name: &str) -> Box<Room> {
    Box::new(Room::new(name))
}

fn recv_message(room: &Room) -> f32 {
    room.recv_message()
}

#[cxx::bridge]
mod ffi {
    extern "Rust" {
        type Room;
        fn create_room(name: &str) -> Box<Room>;
        fn recv_message(room: &Room) -> f32;
    }
}
