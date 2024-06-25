use crate::broadcast_room::BroadcastRoom;
use shared::connections::SteckerWebRTCConnection;
use webrtc::dtls::conn;

use std::sync::{Arc, Mutex};
// use tokio::sync::Mutex;
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

        // map does not support async which is necessary due to async,
        // therefore we use this atrocity
        let mut results = Vec::with_capacity(rooms.len());
        for (uuid, room) in rooms.iter() {
            let locked_room = room.lock().await;
            results.push(Room {
                uuid: uuid.clone(),
                name: locked_room.name.clone(),
                num_listeners: 0,
            });
        }
        Ok(results)
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

        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer).await?;

        let c2 = Arc::new(connection);

        // this is strange? we use a "constructor" method but pass self as an Arc?!
        // is this hacky or is this how you do things in rust? can't arc self within a method
        // therefore we do it like this?
        let mut message_rx = SteckerWebRTCConnection::listen_for_data_channel(
            c2.clone()
        ).await;

        let room = BroadcastRoom {
            name: name,
            source_connection: c2.clone(),
            target_connections: Mutex::new(vec![]),
        };

        let r2 = Arc::new(room);

        tokio::spawn(async move {
            // new idea: use a tokio:spawn to consume two mpsc
            // a) new messages
            // b) new source connections/data channels
            // these connections can then be owned by this thread, making
            // the mutex locks unnecessary :)

            while let Some(msg) = message_rx.recv().await {
                // this works :)
                println!("Received something: {}", msg.clone());
                // for c in r2.target_connections.lock().unwrap().iter() {
                //     c.lock().unwrap().send_data_channel_message(&msg).await;
                // };

            }
        });

        return Ok(response_offer);


        // let mut rooms = state.rooms.lock().await;
        // let mut room = BroadcastRoom::create_new_room(name).await?;

        // let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);



        // let new_connection = Connection::respond_to_offer(offer, Arc::new(tx)).await?;
        // room.source_connection = Some(new_connection.connection);

        // let room_mutex = Arc::new(Mutex::new(room));
        // rooms.insert(uuid.to_string(), room_mutex.clone());

        // tokio::spawn(async move {
        //     while let Some(msg) = rx.recv().await {
        //         for connection in room_mutex
        //             .clone()
        //             .lock()
        //             .await
        //             .target_connections
        //             .lock()
        //             .await
        //             .iter()
        //         {
        //             if let Some(data_connection) = &connection.lock().await.data_channel {
        //                 let _ = data_connection
        //                     .send_text(&msg)
        //                     .await;
        //             }
        //         }
        //     }
        // });

        // Ok(new_connection.offer)
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

        return Ok("".to_string());

        // if let Some(room) = rooms.get_mut(&room_uuid) {
        //     let connection_offer = Connection::respond_to_offer(offer, Arc::new(tx)).await?;
        //     room.lock()
        //         .await
        //         .join_room(connection_offer.connection)
        //         .await;
        //     Ok(connection_offer.offer)
        // } else {
        //     Err("Found no room with the given UUID".into())
        // }
    }
}
