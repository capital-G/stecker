use std::{sync::Arc, time::Duration};

use crate::event_service::RoomEvent;
use futures::future::join_all;
use rand::distributions::{Alphanumeric, DistString};

use anyhow::anyhow;
use shared::models::{RoomFloatData, RoomStringData, API_VERSION};
use tokio::{sync::RwLock, time::sleep};

use async_graphql::{Context, Enum, Object, SimpleObject};
use tracing::{info, instrument, trace, Instrument, Span};
use uuid::Uuid;

use crate::models::dispatcher::{RoomDispatcher, RoomDispatcherInput};
use crate::models::room::{
    BroadcastRoom, ChannelKind, DataChannelKind, RoomCreationReply, RoomType,
};
use crate::AppState;

pub struct Query;

#[derive(SimpleObject)]
pub struct Room {
    name: String,
    uuid: String,
    audio_channel: bool,
    chat_channel: bool,
    float_channel: bool,
    description: String,
    num_listeners: u32,
}

impl Room {
    pub async fn from_broadcast_room(broadcast_room: &BroadcastRoom) -> Self {
        let current_channels = broadcast_room.get_current_channel_types().await;
        Self {
            name: broadcast_room.meta().name.clone(),
            audio_channel: current_channels.contains(&ChannelKind::AudioChannel),
            chat_channel: current_channels
                .contains(&ChannelKind::DataChannel(DataChannelKind::String)),
            float_channel: current_channels
                .contains(&ChannelKind::DataChannel(DataChannelKind::Float)),
            uuid: broadcast_room.meta().uuid.into(),
            description: broadcast_room.meta().description.clone(),
            num_listeners: 0,
        }
    }
}

#[Object]
impl Query {
    async fn api_version<'a>(&self) -> String {
        API_VERSION.to_string()
    }

    async fn rooms<'a>(&self, ctx: &Context<'a>) -> Vec<Room> {
        let state = ctx.data_unchecked::<Arc<AppState>>();

        let room_locks: Vec<_> = {
            let guard = state.rooms.read().await;
            guard.values().cloned().collect()
        };

        join_all(room_locks.into_iter().map(|room_lock| async move {
            let room = room_lock.read().await;
            Room::from_broadcast_room(&*room).await
        }))
        .await
    }

    async fn room_dispatchers<'a>(&self, ctx: &Context<'a>) -> Vec<RoomDispatcher> {
        let state = ctx.data_unchecked::<Arc<AppState>>();

        let guard = state.room_dispatchers.read().await;
        guard.values().cloned().collect()
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
        channel_type: ChannelType,
        password: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<RoomCreationReply> {
        let connection_uuid = Uuid::new_v4();
        tracing::Span::current().record("connection_uuid", connection_uuid.to_string());
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let channel_kind: ChannelKind = channel_type.into();

        if let Some(existing_room) = state.rooms.read().await.get(&name) {
            return Ok(RoomCreationReply {
                offer: existing_room
                    .read()
                    .await
                    .replace_sender(
                        channel_kind,
                        offer,
                        password.clone(),
                        state.room_events.clone(),
                    )
                    .await?,
                password: password.unwrap_or("".to_string()),
            });
        }

        let room_password: String = if let Some(user_provided_password) = password {
            user_provided_password
        } else {
            Alphanumeric.sample_string(&mut rand::thread_rng(), 8)
        };

        let room = Arc::new(RwLock::new(BroadcastRoom::new(
            name.clone(),
            room_password.clone(),
            connection_uuid,
            description.unwrap_or("".to_string()),
        )));
        let room_clone = room.clone();
        state.insert_room(name.clone(), room).await;

        let response = match channel_kind {
            ChannelKind::AudioChannel => {
                room_clone
                    .read()
                    .await
                    .create_audio_channel(&offer, state.room_events.clone())
                    .await
            }
            ChannelKind::DataChannel(kind) => match kind {
                DataChannelKind::Float => {
                    room_clone
                        .read()
                        .await
                        .create_data_channel::<RoomFloatData>(&offer, kind)
                        .await
                }
                DataChannelKind::String => {
                    room_clone
                        .read()
                        .await
                        .create_data_channel::<RoomStringData>(&offer, kind)
                        .await
                }
            },
        }?;

        let room_guard = room_clone.read().await;
        let room_map_clone = state.rooms.clone();
        let mut remove_room = room_guard.free_room.clone().subscribe();
        tokio::spawn(
            async move {
                if let Ok(()) = remove_room.recv().await {
                    info!("Delete room");
                    room_map_clone.write().await.remove(&name);
                }
            }
            .in_current_span(),
        );

        Ok(RoomCreationReply {
            offer: response,
            password: room_password,
        })
    }

    #[instrument(skip(self, ctx, dispatcher), fields(dispatcher_name=dispatcher.name), parent = None, err)]
    async fn create_dispatcher<'a>(
        &self,
        ctx: &Context<'a>,
        dispatcher: RoomDispatcherInput,
    ) -> anyhow::Result<RoomDispatcher> {
        let state = ctx.data_unchecked::<Arc<AppState>>();

        state.create_dispatcher(dispatcher).await
    }

    #[instrument(skip(self, ctx, offer), fields(connection_uuid), parent = None, err)]
    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
        channel_type: ChannelType,
    ) -> anyhow::Result<String> {
        let connection_uuid = Uuid::new_v4();
        tracing::Span::current().record("connection_uuid", connection_uuid.to_string());
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let channel_kind: ChannelKind = channel_type.into();

        match state.rooms.read().await.get(&name) {
            Some(room) => match channel_kind {
                ChannelKind::AudioChannel => room.read().await.join_audio_channel(&offer).await,
                ChannelKind::DataChannel(kind) => match kind {
                    DataChannelKind::Float => {
                        room.read()
                            .await
                            .join_data_channel::<RoomFloatData>(&offer, kind)
                            .await
                    }
                    DataChannelKind::String => {
                        room.read()
                            .await
                            .join_data_channel::<RoomStringData>(&offer, kind)
                            .await
                    }
                },
            },
            None => Err(anyhow::anyhow!("No such room")),
        }
    }

    async fn access_dispatcher<'a>(&self, ctx: &Context<'a>, name: String) -> anyhow::Result<Room> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        todo!()
        /*
        if let Some(dispatcher) = state.room_dispatchers.read().await.get(&name) {
            match dispatcher.room_type {
                RoomType::Float => todo!(),
                RoomType::Chat => todo!(),
                RoomType::Audio => state.audio_rooms.get_room(dispatcher).await,
            }
        } else {
            Err(anyhow!("Could not find a dispatcher with the given name"))
        }
         */
    }
}

// graphql does not allow nested enums, so we have
// to create a flat one which we then convert to a nested one
#[derive(Clone, Copy, Debug, Enum, PartialEq, Eq)]
enum ChannelType {
    Audio,
    Float,
    String,
}

impl From<ChannelType> for ChannelKind {
    fn from(value: ChannelType) -> Self {
        match value {
            ChannelType::Audio => ChannelKind::AudioChannel,
            ChannelType::Float => ChannelKind::DataChannel(DataChannelKind::Float),
            ChannelType::String => ChannelKind::DataChannel(DataChannelKind::String),
        }
    }
}
