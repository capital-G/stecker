use uuid::Uuid;
use crate::models::{BroadcastRoom as BaseRoom, Connection, User};

use async_graphql::{Context, Object, Result, SimpleObject};


use crate::AppState;

pub struct Query;

#[Object]
impl User {
    async fn id(&self) -> &str {
        &self.id
    }

    async fn name(&self) -> &str {
        &self.name
    }
}

#[derive(SimpleObject)]
struct Room {
    uuid: String,
    // #[graphql(flatten)]
    // room: BaseRoom,
    name: String,
    num_listeners: usize,
}



#[Object]
impl Query {
    async fn counter<'a>(
        &self,
        ctx: &Context<'a>,
    ) -> i32 {
        let mut foo = ctx.data_unchecked::<AppState>().counter.lock().await;
        *foo += 1;
        println!("{}", foo);
        return *foo
    }

    async fn rooms<'a>(
        &self,
        ctx: &Context<'a>
    ) -> Result<Vec<Room>> {
        let state = ctx.data_unchecked::<AppState>();
        let rooms = state.rooms.lock().await;
        // let foobar = rooms.values().cloned().collect();
        Ok(rooms.iter().map(|(uuid, room)| {
            Room {
                uuid: uuid.clone(),
                name: room.name.clone(),
                num_listeners: 0,
            }
        }).collect())
    }
}


pub struct Mutation;

#[Object]
impl Mutation {
    async fn counter_offset<'a>(
        &self,
        ctx: &Context<'a>,
        offset: i32,
    ) -> i32 {
        let mut foo = ctx.data_unchecked::<AppState>().counter.lock().await;
        *foo += offset;
        return *foo;
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

        let new_connection = Connection::respond_to_offer(offer).await?;

        room.source_connection = Some(new_connection.connection);
        rooms.insert(uuid.to_string(), room);

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

        if let Some(room) = rooms.get_mut(&room_uuid) {
            // room.listeners.add(rtc peer connection)
            let connection_offer = Connection::respond_to_offer(offer).await?;
            room.join_room(connection_offer.connection).await;
            Ok(connection_offer.offer)
        } else {
            Err("Found no room with the given UUID".into())
        }
        // None => Err("Found no room with the given UUID".into()),
    }
}
