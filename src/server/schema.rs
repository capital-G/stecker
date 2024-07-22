use crate::models::{BroadcastRoom, Room, RoomType};
use shared::models::RoomType as SharedRoomType;

use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use async_graphql::{Context, Object, Result};

use crate::AppState;
pub struct Query;

#[Object]
impl Query {
    async fn rooms<'a>(&self, ctx: &Context<'a>, room_type: RoomType) -> Vec<Room> {
        let state = ctx.data_unchecked::<AppState>();

        match room_type {
            RoomType::Float => state.float_rooms.values().await,
            RoomType::Chat => state.chat_rooms.values().await,
        }
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn reset_rooms<'a>(&self, ctx: &Context<'a>) -> f32 {
        ctx.data_unchecked::<AppState>().reset_rooms().await;
        0.
    }

    async fn create_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        room_type: RoomType,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();

        let shared_room_type: SharedRoomType = room_type.into();
        if state.room_exists(&name, &room_type).await {
            return Err(format!("{shared_room_type} with name {name} already exists.").into());
        }

        let name2 = name.clone();

        match room_type {
            RoomType::Float => {
                let result =
                    BroadcastRoom::<f32>::create_room(name, offer, room_type.into()).await?;
                {
                    let room_mutex = Arc::new(AsyncMutex::new(result.broadcast_room));
                    state.float_rooms.insert(&name2, room_mutex.clone()).await;
                }
                Ok(result.offer)
            }
            RoomType::Chat => {
                let result =
                    BroadcastRoom::<String>::create_room(name, offer, room_type.into()).await?;
                {
                    let room_mutex = Arc::new(AsyncMutex::new(result.broadcast_room));
                    state.chat_rooms.insert(&name2, room_mutex.clone()).await;
                }
                Ok(result.offer)
            }
        }
    }

    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        room_type: RoomType,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();

        // some of duplication b/c room is generic and string/float
        // have different size so they can not be stored in a generic
        // variable. Maybe it is better to replace everything via
        // traits as they can be stored in a box with dynamic size
        // or working with boxes?
        match room_type {
            RoomType::Float => match state.float_rooms.get(&name).await {
                Some(broadcast_room) => {
                    return Ok(broadcast_room.lock().await.join_room(&offer).await?)
                }
                None => return Err(format!("No such room {name}").into()),
            },
            RoomType::Chat => match state.chat_rooms.get(&name).await {
                Some(broadcast_room) => {
                    return Ok(broadcast_room.lock().await.join_room(&offer).await?)
                }
                None => return Err(format!("No such room {name}").into()),
            },
        };
    }
}
