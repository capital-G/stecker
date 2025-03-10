use std::{sync::Arc, time::Duration};

use crate::{
    models::{
        AudioBroadcastRoom, BroadcastRoom, DataBroadcastRoom, Room, RoomCreationReply, RoomType,
    },
    state::RoomMapTrait,
};
use rand::distributions::{Alphanumeric, DistString};

use anyhow::anyhow;
use tokio::sync::Mutex;

use async_graphql::{Context, Object};
use tracing::{info, instrument, trace, Instrument, Span};
use uuid::Uuid;

use crate::AppState;

pub struct Query;

#[Object]
impl Query {
    async fn rooms<'a>(&self, ctx: &Context<'a>, room_type: RoomType) -> Vec<Room> {
        let state = ctx.data_unchecked::<AppState>();

        match room_type {
            RoomType::Float => state.float_rooms.get_rooms().await,
            RoomType::Chat => state.chat_rooms.get_rooms().await,
            RoomType::Audio => state.audio_rooms.get_rooms().await,
        }
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    #[instrument(skip_all, parent = None)]
    async fn reset_rooms<'a>(&self, ctx: &Context<'a>) -> f32 {
        ctx.data_unchecked::<AppState>().reset_rooms().await;
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

        let state = ctx.data_unchecked::<AppState>();

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

        let state = ctx.data_unchecked::<AppState>();

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
}
