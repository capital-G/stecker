use std::{collections::HashMap, future::Future, sync::Arc};

use tokio::sync::Mutex;

use crate::models::{BroadcastRoom, Room, RoomType};

pub struct AppState {
    pub float_rooms: RoomMap,
    pub chat_rooms: RoomMap,
    pub audio_rooms: RoomMap,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            float_rooms: RoomMap {
                map: Arc::new(Mutex::new(HashMap::new())),
            },
            chat_rooms: RoomMap {
                map: Arc::new(Mutex::new(HashMap::new())),
            },
            audio_rooms: RoomMap {
                map: Arc::new(Mutex::new(HashMap::new())),
            },
        }
    }

    pub async fn reset_rooms(&self) {
        self.chat_rooms.reset_state().await;
        self.float_rooms.reset_state().await;
        self.audio_rooms.reset_state().await;
    }

    pub async fn room_exists(&self, room_name: &str, room_type: &RoomType) -> bool {
        match room_type {
            RoomType::Float => self.float_rooms.room_exists(room_name).await,
            RoomType::Chat => self.chat_rooms.room_exists(room_name).await,
            RoomType::Audio => self.audio_rooms.room_exists(room_name).await,
        }
    }

    pub async fn room_password_match(
        &self,
        room_name: &str,
        room_type: &RoomType,
        password: &str,
    ) -> bool {
        match room_type {
            RoomType::Audio => {
                self.audio_rooms
                    .room_password_match(room_name, password)
                    .await
            }
            RoomType::Chat => {
                self.chat_rooms
                    .room_password_match(room_name, password)
                    .await
            }
            RoomType::Float => {
                self.float_rooms
                    .room_password_match(room_name, password)
                    .await
            }
        }
    }
}

pub struct RoomMap {
    pub map: Arc<Mutex<HashMap<String, Arc<Mutex<BroadcastRoom>>>>>,
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
    fn room_password_match(
        &self,
        room_name: &str,
        room_password_match: &str,
    ) -> impl Future<Output = bool>;
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

    async fn room_password_match(&self, room_name: &str, password: &str) -> bool {
        if let Some(room) = self.map.lock().await.get(room_name) {
            room.lock().await.meta().admin_password == password
        } else {
            false
        }
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
