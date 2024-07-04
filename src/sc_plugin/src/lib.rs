use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tokio;
use tokio::runtime::Runtime;
use tokio::time::sleep;

pub struct Room {
    name: String,
    next_float: Arc<Mutex<f32>>,
}

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
