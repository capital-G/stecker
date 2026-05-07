use futures::stream::{self, StreamExt};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tracing::{info, Instrument};

use minijinja;
use tokio::{sync::RwLock, time::sleep};

use crate::{
    event_service::RoomEvent,
    models::{BroadcastRoom, RoomDispatcher, RoomDispatcherInput, RoomType},
};

pub struct AppState {
    pub rooms: Arc<RwLock<HashMap<String, Arc<RwLock<BroadcastRoom>>>>>,
    pub room_dispatchers: Arc<RwLock<HashMap<String, RoomDispatcher>>>,

    pub room_events: tokio::sync::broadcast::Sender<RoomEvent>,
    pub jinja: Arc<minijinja::Environment<'static>>,
}

/// The state of the app. Any kind of access or mutation of the state
/// needs to happen through methods such that all necessary events
/// will be triggered.
impl AppState {
    pub fn new() -> Self {
        let mut env = minijinja::Environment::new();
        let template_dir = std::env::current_dir().unwrap().join("templates");
        env.set_loader(minijinja::path_loader(template_dir));

        let (room_event_rx, _) = tokio::sync::broadcast::channel(32);
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            room_dispatchers: Arc::new(RwLock::new(HashMap::new())),
            room_events: room_event_rx,
            jinja: Arc::new(env),
        }
    }

    pub async fn insert_room(&self, name: String, room: Arc<RwLock<BroadcastRoom>>) {
        self.rooms.write().await.insert(name, room);
    }

    pub async fn reset_rooms(&self) {
        let _ = self.room_events.send(RoomEvent::RoomDispatcherReset());
        self.rooms.write().await.clear();
    }

    pub async fn room_exists(&self, room_name: &str) -> bool {
        self.rooms.read().await.contains_key(room_name)
    }

    // pub async fn replace_audio_sender(
    //     &self,
    //     room_name: &str,
    //     password: &str,
    //     offer: &str,
    // ) -> anyhow::Result<String> {
    //     if let Some(room) = self.rooms.read().await.get(room_name).cloned() {
    //         if room.read().await.meta().admin_password == password {
    //             let _ = self.room_events.send(RoomEvent::BroadcastRoomUpdated(
    //                 room.read().await.meta().name.clone(),
    //             ));
    //             room.write().await.replace_sender(offer, password).await
    //         } else {
    //             Err(anyhow::anyhow!("Password does not match"))
    //         }
    //     } else {
    //         Err(anyhow::anyhow!("Did not find room"))
    //     }
    // }

    pub async fn room_password_match(&self, room_name: &str, password: &str) -> bool {
        match self.rooms.read().await.get(room_name) {
            Some(room) => room.read().await.meta().admin_password == password,
            None => false,
        }
    }

    pub async fn create_dispatcher(
        &self,
        dispatcher_input: RoomDispatcherInput,
    ) -> anyhow::Result<RoomDispatcher> {
        let name = dispatcher_input.name.clone();
        let admin_password = dispatcher_input.admin_password.clone();
        let timeout_value = dispatcher_input.timeout;

        let room_dispatcher: RoomDispatcher = dispatcher_input.clone().into();

        if let Some(existing_dispatcher) = self.room_dispatchers.write().await.get_mut(&name) {
            return match admin_password {
                None => Err(anyhow::anyhow!(
                    "Dispatcher already exists and no password provided"
                )),
                Some(given_password) => {
                    match given_password == existing_dispatcher.admin_password {
                        false => Err(anyhow::anyhow!(
                            "Password of existing dispatcher does not match"
                        )),
                        true => {
                            existing_dispatcher.rule = room_dispatcher.rule;
                            let _ = existing_dispatcher
                                .timeout_sender
                                .send(Duration::from_secs(timeout_value.try_into()?));
                            Ok(existing_dispatcher.clone())
                        }
                    }
                }
            };
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

        let dispatcher_map = self.room_dispatchers.clone();
        let name = name.clone();
        let room_events = self.room_events.clone();
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
                let _ = room_events.send(RoomEvent::BroadcastRoomDeleted(name.clone()));
                dispatcher_map.write().await.remove(&name);
            }
            .in_current_span(),
        );

        Ok(room_dispatcher)
    }

    pub async fn get_room(&self, dispatcher: &RoomDispatcher) -> anyhow::Result<BroadcastRoom> {
        let rooms_guard = self.rooms.read().await;

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
