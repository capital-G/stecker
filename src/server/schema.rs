use std::{sync::Arc, time::Duration};

use crate::{
    models::{
        AudioBroadcastRoom, BroadcastRoom, DataBroadcastRoom, Room, RoomCreationReply,
        RoomDispatcher, RoomDispatcherInput, RoomType,
    },
    state::RoomMapTrait,
};
use rand::distributions::{Alphanumeric, DistString};

use anyhow::anyhow;
use shared::models::API_VERSION;
use tokio::{sync::Mutex, time::sleep};

use async_graphql::{Context, Object};
use tracing::{info, instrument, trace, Instrument, Span};
use uuid::Uuid;

use crate::AppState;

pub struct Query;

#[Object]
impl Query {
    async fn api_version<'a>(&self) -> String {
        API_VERSION.to_string()
    }

    async fn rooms<'a>(&self, ctx: &Context<'a>, room_type: RoomType) -> Vec<Room> {
        let state = ctx.data_unchecked::<Arc<AppState>>();

        match room_type {
            RoomType::Float => state.float_rooms.get_rooms().await,
            RoomType::Chat => state.chat_rooms.get_rooms().await,
            RoomType::Audio => state.audio_rooms.get_rooms().await,
        }
    }

    async fn room_dispatchers<'a>(&self, ctx: &Context<'a>) -> Vec<RoomDispatcher> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        state
            .room_dispatchers
            .lock()
            .await
            .values()
            .map(|x| x.clone())
            .collect()
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    #[instrument(skip_all, parent = None)]
    async fn reset_rooms<'a>(&self, ctx: &Context<'a>) -> f32 {
        ctx.data_unchecked::<Arc<AppState>>().reset_rooms().await;
        info!("Resetted rooms");
        0.
    }

    #[instrument(skip(self, ctx, offer, password), fields(connection_uuid), parent = None, err)]
    async fn create_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        room_type: RoomType,
        password: Option<String>,
    ) -> anyhow::Result<RoomCreationReply> {
        let connection_uuid = Uuid::new_v4();
        tracing::Span::current().record("connection_uuid", connection_uuid.to_string());

        let state = ctx.data_unchecked::<Arc<AppState>>();

        if state.room_exists(&name, &room_type).await {
            if let Some(user_provided_password) = password {
                if state
                    .room_password_match(&name, &room_type, &user_provided_password)
                    .await
                {
                    trace!("Matched password of existing room");
                    let offer = state
                        .replace_sender(&name, &room_type, &user_provided_password, &offer)
                        .await?;

                    return Ok(RoomCreationReply {
                        offer,
                        password: user_provided_password,
                    });
                }
            };
            return Err(anyhow!("The room name is already taken."));
        }

        let room_password: String = if let Some(user_provided_password) = password {
            user_provided_password
        } else {
            Alphanumeric.sample_string(&mut rand::thread_rng(), 8)
        };

        let name2 = name.clone();
        let room_password2 = room_password.clone();
        match room_type {
            RoomType::Float | RoomType::Chat => {
                let result =
                    DataBroadcastRoom::create_room(name, offer, room_type.into(), room_password)
                        .instrument(Span::current())
                        .await?;
                {
                    let mut room_lock = match room_type {
                        RoomType::Float => {
                            info!("Created a float room");
                            state.float_rooms.map.lock().await
                        }
                        RoomType::Chat => {
                            info!("Created a chat room");
                            state.chat_rooms.map.lock().await
                        }
                        RoomType::Audio => {
                            todo!("This can not happen - can we inherit the types from above?")
                        }
                    };
                    let room_mutex =
                        Arc::new(Mutex::new(BroadcastRoom::Data(result.broadcast_room)));
                    room_lock.insert(name2, room_mutex.clone());
                }
                Ok(RoomCreationReply {
                    offer: result.offer,
                    password: room_password2,
                })
            }
            RoomType::Audio => {
                let result = AudioBroadcastRoom::create_room(name, offer, room_password)
                    .in_current_span()
                    .await?;
                {
                    let mut room_lock = match room_type {
                        RoomType::Audio => state.audio_rooms.map.lock().await,
                        _ => {
                            todo!("This can not happen - can we inherit the types from above?")
                        }
                    };

                    let mut stream_sequence_number = result
                        .audio_broadcast_room
                        .stecker_audio_channel
                        .sequence_number_receiver
                        .clone();

                    let room_mutex = Arc::new(Mutex::new(BroadcastRoom::Audio(
                        result.audio_broadcast_room,
                    )));
                    let name3 = name2.clone();
                    room_lock.insert(name2, room_mutex.clone());

                    let audio_room_mutex = state.audio_rooms.map.clone();

                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                _ = stream_sequence_number.changed() => {},
                                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                                    info!("Timeout for not receiving any package from the sender");
                                    break;
                                }
                            }
                        }
                        let mut audio_room_mutex_lock = audio_room_mutex.lock().await;
                        audio_room_mutex_lock.remove(&name3);
                        info!("Cleared room");
                    }
                    .in_current_span(),);
                }
                info!("Created an audio room");

                Ok(RoomCreationReply {
                    offer: result.offer,
                    password: room_password2,
                })
            }
        }
    }

    #[instrument(skip(self, ctx, dispatcher), fields(dispatcher_name=dispatcher.name), parent = None, err)]
    async fn create_dispatcher<'a>(
        &self,
        ctx: &Context<'a>,
        dispatcher: RoomDispatcherInput,
    ) -> anyhow::Result<RoomDispatcher> {
        let name = dispatcher.name.clone();
        let admin_password = dispatcher.admin_password.clone();
        let timeout_value = dispatcher.timeout;

        let room_dispatcher: RoomDispatcher = dispatcher.into();
        let state = ctx.data_unchecked::<Arc<AppState>>();

        if let Some(existing_dispatcher) = state.room_dispatchers.lock().await.get_mut(&name) {
            if let Some(pw) = admin_password {
                if pw == existing_dispatcher.admin_password {
                    existing_dispatcher.rule = room_dispatcher.rule;
                    let _ = existing_dispatcher
                        .timeout_sender
                        .send(Duration::from_secs(timeout_value));
                    return Ok(existing_dispatcher.clone());
                } else {
                    return Err(anyhow!("Password of existing dispatcher does not match"));
                }
            } else {
                return Err(anyhow!(
                    "Dispatcher already exists and no password provided"
                ));
            }
        };

        let mut timeout_receiver = room_dispatcher.timeout_receiver.clone();

        state
            .room_dispatchers
            .lock()
            .await
            .insert(room_dispatcher.name.clone(), room_dispatcher.clone());
        info!("Created a new dispatcher");

        let dispatcher_map_lock = state.room_dispatchers.clone();
        let name2 = name.clone();
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
                dispatcher_map_lock.lock().await.remove(&name2);
            }
            .in_current_span(),
        );

        Ok(room_dispatcher)
    }

    #[instrument(skip(self, ctx, offer), fields(connection_uuid), parent = None, err)]
    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        room_type: RoomType,
    ) -> anyhow::Result<String> {
        let connection_uuid = Uuid::new_v4();
        tracing::Span::current().record("connection_uuid", connection_uuid.to_string());

        info!("Join room");

        let state = ctx.data_unchecked::<Arc<AppState>>();

        match room_type {
            RoomType::Float => match state.float_rooms.map.lock().await.get(&name) {
                Some(broadcast_room) => Ok(broadcast_room
                    .lock()
                    .await
                    .join_room(&offer)
                    .instrument(Span::current())
                    .await?),
                None => Err(anyhow!("No such room {name}")),
            },
            RoomType::Chat => match state.chat_rooms.map.lock().await.get(&name) {
                Some(broadcast_room) => Ok(broadcast_room
                    .lock()
                    .await
                    .join_room(&offer)
                    .instrument(Span::current())
                    .await?),
                None => Err(anyhow!("No such room {name}")),
            },
            RoomType::Audio => match state.audio_rooms.map.lock().await.get(&name) {
                Some(broadcast_room) => Ok(broadcast_room
                    .lock()
                    .await
                    .join_room(&offer)
                    .instrument(Span::current())
                    .await?),
                None => Err(anyhow!("No such room {name}")),
            },
        }
    }

    async fn access_dispatcher<'a>(&self, ctx: &Context<'a>, name: String) -> anyhow::Result<Room> {
        let state = ctx.data_unchecked::<Arc<AppState>>();

        if let Some(dispatcher) = state.room_dispatchers.lock().await.get(&name) {
            match dispatcher.room_type {
                RoomType::Float => todo!(),
                RoomType::Chat => todo!(),
                RoomType::Audio => state.audio_rooms.get_room(dispatcher).await,
            }
        } else {
            Err(anyhow!("Could not find a dispatcher with the given name"))
        }
    }
}
