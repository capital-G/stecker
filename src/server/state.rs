use futures::stream::{self, StreamExt};
use minijinja_autoreload::AutoReloader;
use std::{collections::HashMap, future::Future, path::PathBuf, sync::Arc, time::Duration};
use tracing::{info, Instrument};

use minijinja::{self, path_loader, Environment};
use tokio::{sync::RwLock, time::sleep};

use crate::{
    event_service::RoomEvent,
    models::{BroadcastRoom, Room, RoomDispatcher, RoomDispatcherInput, RoomType},
};

pub struct AppState {
    pub float_rooms: RoomMap,
    pub chat_rooms: RoomMap,
    pub audio_rooms: RoomMap,
    pub room_dispatchers: Arc<RwLock<HashMap<String, RoomDispatcher>>>,

    pub room_events: tokio::sync::broadcast::Sender<RoomEvent>,
    pub jinja: Arc<minijinja::Environment<'static>>,
    pub jinja_reloader: AutoReloader,
}

impl AppState {
    pub fn new() -> Self {
        let mut env = minijinja::Environment::new();
        let template_dir = std::env::current_dir().unwrap().join("templates");
        env.set_loader(minijinja::path_loader(template_dir.clone()));

        let reloader = AutoReloader::new(move |notifier| {
            let mut env = Environment::new();
            env.set_loader(path_loader(&template_dir));

            notifier.watch_path(&template_dir, true);

            Ok(env)
        });

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
            jinja_reloader: reloader,
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

    pub async fn create_dispatcher(
        &self,
        dispatcher_input: RoomDispatcherInput,
    ) -> anyhow::Result<RoomDispatcher> {
        let name = dispatcher_input.name.clone();
        let admin_password = dispatcher_input.admin_password.clone();
        let timeout_value = dispatcher_input.timeout;

        let room_dispatcher: RoomDispatcher = dispatcher_input.into();

        if let Some(existing_dispatcher) = self.room_dispatchers.write().await.get_mut(&name) {
            if let Some(pw) = admin_password {
                if pw == existing_dispatcher.admin_password {
                    existing_dispatcher.rule = room_dispatcher.rule;
                    let _ = existing_dispatcher
                        .timeout_sender
                        .send(Duration::from_secs(timeout_value.try_into()?));
                    return Ok(existing_dispatcher.clone());
                } else {
                    return Err(anyhow::anyhow!(
                        "Password of existing dispatcher does not match"
                    ));
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Dispatcher already exists and no password provided"
                ));
            }
        };

        let mut timeout_receiver = room_dispatcher.timeout_receiver.clone();

        self.room_dispatchers
            .write()
            .await
            .insert(room_dispatcher.name.clone(), room_dispatcher.clone());
        info!("Created a new dispatcher");

        let _ = self.room_events.send(RoomEvent::RoomDispatcherCreated(
            room_dispatcher.name.clone(),
        ));

        let dispatcher_map_lock = self.room_dispatchers.clone();
        let name2 = name.clone();
        let dispatcher_deleted_event = self.room_events.clone();
        tokio::spawn(
            async move {
                loop {
                    let duration = *timeout_receiver.borrow();
                    tokio::select! {
                        _ = timeout_receiver.changed() => {}
                        _ = sleep(duration) => {break}
                    }
                }
                info!("Dispatcher timed out - will be deleted now");
                let _ =
                    dispatcher_deleted_event.send(RoomEvent::BroadcastRoomDeleted(name2.clone()));
                dispatcher_map_lock.write().await.remove(&name2);
            }
            .in_current_span(),
        );

        Ok(room_dispatcher)
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

        dispatcher
            .dispatcher_type
            .choose_room(matched_rooms)
            .await
            .ok_or(anyhow::anyhow!("Could not find matching room"))
    }
}
