use std::sync::Arc;

use crate::models::{BroadcastRoom as BaseRoom, Connection};
use tokio::sync::Mutex;
use uuid::Uuid;

use async_graphql::{Context, Object, Result, SimpleObject};

use crate::AppState;

pub struct Query;

#[derive(SimpleObject)]
struct Room {
    uuid: String,
    name: String,
    num_listeners: usize,
}

#[Object]
impl Query {
    async fn counter<'a>(&self, ctx: &Context<'a>) -> i32 {
        let mut counter = ctx.data_unchecked::<AppState>().counter.lock().await;
        *counter += 1;
        *counter
    }

    async fn rooms<'a>(&self, ctx: &Context<'a>) -> Result<Vec<Room>> {
        let state = ctx.data_unchecked::<AppState>();
        let rooms = state.rooms.lock().await;

        Ok(rooms
            .iter()
            .map(|(uuid, _room)| {
                Room {
                    uuid: uuid.clone(),
                    // @todo async mutex makes problems here
                    name: "Hello".to_string(),
                    num_listeners: 0,
                }
            })
            .collect())
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn counter_offset<'a>(&self, ctx: &Context<'a>, offset: i32) -> i32 {
        let mut foo = ctx.data_unchecked::<AppState>().counter.lock().await;
        *foo += offset;
        *foo
    }

    async fn create_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
    ) -> Result<String> {
        let uuid = Uuid::new_v4();
        let state = ctx.data_unchecked::<AppState>();
        let mut rooms = state.rooms.lock().await;
        let mut room = BaseRoom::create_new_room(name).await?;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

        let new_connection = Connection::respond_to_offer(offer, Arc::new(tx)).await?;
        room.source_connection = Some(new_connection.connection);

        let room_mutex = Arc::new(Mutex::new(room));
        rooms.insert(uuid.to_string(), room_mutex.clone());

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                for connection in room_mutex
                    .clone()
                    .lock()
                    .await
                    .target_connections
                    .lock()
                    .await
                    .iter()
                {
                    if let Some(data_connection) = &connection.lock().await.data_channel {
                        let _ = data_connection
                            .send_text(&msg)
                            .await;
                    }
                }
            }
        });

        Ok(new_connection.offer)
    }

    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        room_uuid: String,
        offer: String,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();
        let mut rooms = state.rooms.lock().await;

        let (tx, _rx) = tokio::sync::mpsc::channel::<String>(1);

        if let Some(room) = rooms.get_mut(&room_uuid) {
            let connection_offer = Connection::respond_to_offer(offer, Arc::new(tx)).await?;
            room.lock()
                .await
                .join_room(connection_offer.connection)
                .await;
            Ok(connection_offer.offer)
        } else {
            Err("Found no room with the given UUID".into())
        }
    }
}
