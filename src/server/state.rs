use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;

use crate::models::{BroadcastRoom, Room, RoomType};

pub struct AppState {
    // in order to avoid dynamic dispatching which involves lots of
    // code duplication due to pattern matching or creating a shared
    // generic trait which could be boxed, we simply create an
    // HashMap for each room type we have
    pub float_rooms: RoomMap<BroadcastRoom<f32>>,
    pub chat_rooms: RoomMap<BroadcastRoom<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            float_rooms: RoomMap(Mutex::new(HashMap::new())),
            chat_rooms: RoomMap(Mutex::new(HashMap::new())),
        }
    }

    pub async fn reset_rooms(&self) {
        self.chat_rooms.clear().await;
        self.float_rooms.clear().await;
    }

    pub async fn room_exists(&self, room_name: &str, room_type: &RoomType) -> bool {
        match room_type {
            RoomType::Float => self.float_rooms.contains_key(room_name).await,
            RoomType::Chat => self.chat_rooms.contains_key(room_name).await,
        }
    }
}

pub struct RoomMap<R: Clone>(Mutex<HashMap<String, Arc<Mutex<R>>>>);

impl<R: Clone> RoomMap<R> {
    pub async fn clear(&self) {
        let mut map_lock = self.0.lock().await;
        *map_lock = HashMap::new();
    }

    pub async fn insert(&self, room_name: &str, room: Arc<Mutex<R>>) {
        self.0.lock().await.insert(room_name.to_string(), room);
    }

    pub async fn get(&self, room_name: &str) -> Option<Arc<Mutex<R>>> {
        self.0.lock().await.get(room_name).map(|a| a.clone())
    }

    pub async fn contains_key(&self, room_name: &str) -> bool {
        self.0.lock().await.contains_key(room_name)
    }

    pub async fn values(&self) -> Vec<R> {
        let map_lock = self.0.lock().await;

        let mut rooms = Vec::new();

        for (_, room_arc) in map_lock.iter() {
            let room_lock = room_arc.lock().await;
            rooms.push((*room_lock).clone());
        }
        rooms
    }
}
