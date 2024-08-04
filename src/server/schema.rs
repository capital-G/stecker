use crate::{
    models::{AudioBroadcastRoom, BroadcastRoom, DataBroadcastRoom, Room, RoomType},
    state::RoomMapTrait,
};
use shared::models::DataRoomInternalType;

use std::sync::Arc;
use tokio::sync::Mutex;

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
            RoomType::Audio => state.audio_rooms.get_rooms().await,
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

        if state.room_exists(&name, &room_type).await {
            return Err(format!("{room_type} with name {name} already exists.").into());
        }

        let name2 = name.clone();
        println!("Created new room {name2}");

        match room_type {
            RoomType::Float | RoomType::Chat => {
                let result = DataBroadcastRoom::create_room(name, offer, room_type.into()).await?;
                {
                    let mut room_lock = match room_type {
                        RoomType::Float => state.float_rooms.map.lock().await,
                        RoomType::Chat => state.chat_rooms.map.lock().await,
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
                let result = AudioBroadcastRoom::create_room(name, offer).await?;
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
                Ok(result.offer)
            }
        }
    }

    // async fn playback_audio_room<'a>(
    //     &self,
    //     ctx: &Context<'a>,
    //     offer: String
    // ) -> Result<String> {
    //     let result = AudioRoom::playback_room(offer).await?;
    //     Ok(result.offer)
    // }

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
            RoomType::Audio => match state.audio_rooms.map.lock().await.get(&name) {
                Some(broadcast_room) => Ok(broadcast_room.lock().await.join_room(&offer).await?),
                None => Err(format!("No such room {name}").into()),
            },
        }
    }
}
