use crate::{
    models::{BroadcastRoom, Room, RoomType},
    state::RoomMapTrait,
};
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
            RoomType::Float => state.float_rooms.get_rooms().await,
            RoomType::Chat => state.chat_rooms.get_rooms().await,
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
        println!("Created new room {name2}");

        let result = BroadcastRoom::create_room(name, offer, room_type.into()).await?;
        {
            let mut room_lock = match room_type {
                RoomType::Float => state.float_rooms.map.lock().await,
                RoomType::Chat => state.chat_rooms.map.lock().await,
            };
            let room_mutex = Arc::new(AsyncMutex::new(result.broadcast_room));
            room_lock.insert(name2, room_mutex.clone());
        }
        Ok(result.offer)
    }

    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        room_type: RoomType,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();

        match room_type {
            RoomType::Float => match state.float_rooms.map.lock().await.get(&name) {
                Some(broadcast_room) => Ok(broadcast_room.lock().await.join_room(&offer).await?),
                None => Err(format!("No such room {name}").into()),
            },
            RoomType::Chat => match state.chat_rooms.map.lock().await.get(&name) {
                Some(broadcast_room) => Ok(broadcast_room.lock().await.join_room(&offer).await?),
                None => Err(format!("No such room {name}").into()),
            },
        }
    }
}
