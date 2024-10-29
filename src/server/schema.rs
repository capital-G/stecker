use std::sync::Arc;

use crate::{
    models::{AudioBroadcastRoom, BroadcastRoom, DataBroadcastRoom, Room, RoomType},
    state::RoomMapTrait,
};

use anyhow::anyhow;
use tokio::sync::Mutex;

use async_graphql::{Context, Object};
use tracing::{info, instrument, Instrument, Span};
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

    #[instrument(skip(self, ctx, offer), fields(connection_uuid), parent = None, err)]
    async fn create_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        room_type: RoomType,
    ) -> anyhow::Result<String> {
        let connection_uuid = Uuid::new_v4();
        tracing::Span::current().record("connection_uuid", connection_uuid.to_string());

        let state = ctx.data_unchecked::<AppState>();

        if state.room_exists(&name, &room_type).await {
            return Err(anyhow!("The room name is already taken."));
        }

        let name2 = name.clone();
        match room_type {
            RoomType::Float | RoomType::Chat => {
                let result = DataBroadcastRoom::create_room(name, offer, room_type.into())
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
                Ok(result.offer)
            }
            RoomType::Audio => {
                let result = AudioBroadcastRoom::create_room(name, offer)
                    .instrument(Span::current())
                    .await?;
                {
                    let mut room_lock = match room_type {
                        RoomType::Audio => state.audio_rooms.map.lock().await,
                        _ => {
                            todo!("This can not happen - can we inherit the types from above?")
                        }
                    };
                    let room_mutex = Arc::new(Mutex::new(BroadcastRoom::Audio(
                        result.audio_broadcast_room,
                    )));
                    room_lock.insert(name2, room_mutex.clone());
                }
                info!("Created an audio room");
                Ok(result.offer)
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
