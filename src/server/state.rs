use futures::stream::{self, StreamExt};
use std::{collections::HashMap, future::Future, path::PathBuf, sync::Arc};

use minijinja;
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use tokio::sync::RwLock;

use crate::{
    event_service::RoomEvent,
    models::{BroadcastRoom, Room, RoomDispatcher, RoomType},
};

pub struct AppState {
    pub float_rooms: RoomMap,
    pub chat_rooms: RoomMap,
    pub audio_rooms: RoomMap,
    pub room_dispatchers: Arc<RwLock<HashMap<String, RoomDispatcher>>>,

    pub room_events: tokio::sync::broadcast::Sender<RoomEvent>,
    pub jinja: Arc<minijinja::Environment<'static>>,
}

impl AppState {
    pub fn new() -> Self {
        let mut env = minijinja::Environment::new();
        let template_dir = std::env::current_dir().unwrap().join("templates");
        env.set_loader(minijinja::path_loader(template_dir));

        let (room_event_rx, _) = tokio::sync::broadcast::channel(32);
        Self {
            float_rooms: RoomMap {
                map: Arc::new(RwLock::new(HashMap::new())),
                room_events: room_event_rx.clone(),
            },
            chat_rooms: RoomMap {
                map: Arc::new(RwLock::new(HashMap::new())),
                room_events: room_event_rx.clone(),
            },
            audio_rooms: RoomMap {
                map: Arc::new(RwLock::new(HashMap::new())),
                room_events: room_event_rx.clone(),
            },
            room_dispatchers: Arc::new(RwLock::new(HashMap::new())),
            room_events: room_event_rx,
            jinja: Arc::new(env),
        }
    }

    pub async fn reset_rooms(&self) {
        let _ = self.room_events.send(RoomEvent::RoomDispatcherReset());
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

    pub async fn replace_sender(
        &self,
        room_name: &str,
        room_type: &RoomType,
        password: &str,
        offer: &str,
    ) -> anyhow::Result<String> {
        match room_type {
            RoomType::Audio => {
                self.audio_rooms
                    .replace_sender(room_name, offer, password)
                    .await
            }
            RoomType::Float => todo!(),
            RoomType::Chat => todo!(),
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
    pub map: Arc<RwLock<HashMap<String, Arc<RwLock<BroadcastRoom>>>>>,

    room_events: tokio::sync::broadcast::Sender<RoomEvent>,
}

pub trait RoomMapTrait {
    fn reset_state(&self) -> impl Future<Output = ()>;
    fn insert_room(
        &self,
        room_name: &str,
        room: Arc<RwLock<BroadcastRoom>>,
    ) -> impl Future<Output = ()>;
    fn room_exists(&self, room_name: &str) -> impl Future<Output = bool>;
    fn get_rooms(&self) -> impl Future<Output = Vec<Room>>;
    fn room_password_match(
        &self,
        room_name: &str,
        room_password_match: &str,
    ) -> impl Future<Output = bool>;

    fn replace_sender(
        &self,
        room_name: &str,
        offer: &str,
        password: &str,
    ) -> impl Future<Output = anyhow::Result<String>>;

    fn get_room(&self, dispatcher: &RoomDispatcher) -> impl Future<Output = anyhow::Result<Room>>;
}

impl RoomMapTrait for RoomMap {
    async fn reset_state(&self) {
        let mut map_lock = self.map.write().await;
        *map_lock = HashMap::new();
    }

    async fn insert_room(&self, room_name: &str, room: Arc<RwLock<BroadcastRoom>>) {
        let _ = self
            .room_events
            .send(RoomEvent::BroadcastRoomCreated(room_name.to_string()));
        self.map.write().await.insert(room_name.to_string(), room);
    }

    async fn room_exists(&self, room_name: &str) -> bool {
        self.map.read().await.contains_key(room_name)
    }

    async fn room_password_match(&self, room_name: &str, password: &str) -> bool {
        if let Some(room) = self.map.read().await.get(room_name) {
            room.read().await.meta().admin_password == password
        } else {
            false
        }
    }

    async fn get_rooms(&self) -> Vec<Room> {
        let rooms: Vec<_> = {
            let map_lock = self.map.read().await;
            map_lock.values().cloned().collect()
        };

        let mut result = Vec::with_capacity(rooms.len());

        for room in rooms {
            let room_lock = room.read().await;
            result.push((&*room_lock).into());
        }

        result
    }

    async fn replace_sender(
        &self,
        room_name: &str,
        offer: &str,
        password: &str,
    ) -> anyhow::Result<String> {
        if let Some(room) = self.map.read().await.get(room_name).cloned() {
            // let room_clone = room.clone();
            if room.read().await.meta().admin_password == password {
                let _ = self.room_events.send(RoomEvent::BroadcastRoomUpdated(
                    room.read().await.meta().name.clone(),
                ));
                room.write().await.replace_sender(offer, password).await
            } else {
                return Err(anyhow::anyhow!("Password does not match"));
            }
        } else {
            return Err(anyhow::anyhow!("Did not find room"));
        }
    }

    async fn get_room(&self, dispatcher: &RoomDispatcher) -> anyhow::Result<Room> {
        let rooms_guard = self.map.read().await;

        // is this the proper way to do this?
        let matched_rooms: Vec<_> = stream::iter(rooms_guard.values())
            .filter_map(|room| async {
                let room_guard = room.read().await;
                if dispatcher.rule.is_match(&room_guard.meta().name) {
                    Some(room.clone())
                } else {
                    None
                }
            })
            .collect()
            .await;

        match dispatcher.dispatcher_type {
            crate::models::DispatcherType::Random => {
                if let Some(room) = matched_rooms.choose(&mut StdRng::from_entropy()) {
                    let room_lock = room.read().await;
                    return Ok((&*room_lock).into());
                } else {
                    return Err(anyhow::anyhow!("Did not match any room"));
                }
            }
        }
    }
}
