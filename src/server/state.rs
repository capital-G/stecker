use std::{collections::HashMap, future::Future, sync::Arc};

use tokio::sync::Mutex;

use crate::models::{BroadcastRoom, Room, RoomType};

pub struct AppState {
    pub float_rooms: RoomMap,
    pub chat_rooms: RoomMap,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            float_rooms: RoomMap {
                map: Mutex::new(HashMap::new()),
            },
            chat_rooms: RoomMap {
                map: Mutex::new(HashMap::new()),
            },
        }
    }

    pub async fn reset_rooms(&self) {
        self.chat_rooms.reset_state().await;
        self.float_rooms.reset_state().await;
    }

    pub async fn room_exists(&self, room_name: &str, room_type: &RoomType) -> bool {
        match room_type {
            RoomType::Float => self.float_rooms.room_exists(room_name).await,
            RoomType::Chat => self.chat_rooms.room_exists(room_name).await,
        }
    }
}

pub struct RoomMap {
    pub map: Mutex<HashMap<String, Arc<Mutex<BroadcastRoom>>>>,
}

pub trait RoomMapTrait {
    fn reset_state(&self) -> impl Future<Output = ()>;
    fn insert_room(
        &self,
        room_name: &str,
        room: Arc<Mutex<BroadcastRoom>>,
    ) -> impl Future<Output = ()>;
    fn room_exists(&self, room_name: &str) -> impl Future<Output = bool>;
    fn get_rooms(&self) -> impl Future<Output = Vec<Room>>;
}

impl RoomMapTrait for RoomMap {
    async fn reset_state(&self) {
        let mut map_lock = self.map.lock().await;
        *map_lock = HashMap::new();
    }

    async fn insert_room(&self, room_name: &str, room: Arc<Mutex<BroadcastRoom>>) {
        self.map.lock().await.insert(room_name.to_string(), room);
    }

    async fn room_exists(&self, room_name: &str) -> bool {
        self.map.lock().await.contains_key(room_name)
    }

    async fn get_rooms(&self) -> Vec<Room> {
        let map_lock = self.map.lock().await;

        let mut rooms: Vec<Room> = Vec::with_capacity(map_lock.len());
        for (_, room_arc) in map_lock.iter() {
            let room_lock = room_arc.lock().await;
            let room: Room = (&*room_lock).into();
            rooms.push(room.clone());
        }
        rooms
    }
}
