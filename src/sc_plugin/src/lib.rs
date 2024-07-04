pub struct Room {
    name: String,
}

impl Room {
    pub fn new(name: &str) -> Self {
        Room {
            name: name.to_string(),
        }
    }

    pub fn recv_message(&self) -> f32 {
        let name = &self.name;
        println!("room name is {name}");
        32.
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
