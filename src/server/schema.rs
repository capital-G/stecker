use crate::broadcast_room::BroadcastRoom;
use shared::connections::{listen_for_data_channel, SteckerWebRTCConnection};
use webrtc::dtls::conn;

use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
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

        let (reply, broadcast) = listen_for_data_channel(&connection.peer_connection).await;

        let c2 = Arc::new(connection);

        let room = BroadcastRoom {
            name: name,
            reply: reply.clone(),
            broadcast,
            source_connection: c2.clone(),
            target_connections: Mutex::new(vec![]),
        };

        let mut rooms = state.rooms.lock().await;
        
        let room_mutex = Arc::new(AsyncMutex::new(room));
        rooms.insert(uuid.to_string(), room_mutex.clone());
        

        tokio::spawn(async move {
            let mut room_receiver = reply.subscribe();
            
            while let Ok(msg) = room_receiver.recv().await {
                println!("Received something from a target (?): {}", msg.clone());
            }
        });

        return Ok(response_offer);
    }

    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        room_uuid: String,
        offer: String,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();
        let mut rooms = state.rooms.lock().await;
        let  room = rooms.get_mut(&room_uuid);

        if room.is_none() {
            return Err("No such room {room_uuid}".into());
        }

        let room = room.unwrap().lock().await;
        
        let mut room_rx = room.broadcast.subscribe();
        let room_tx = room.reply.clone();

        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer).await?;
        let (reply, broadcast) = listen_for_data_channel(&connection.peer_connection).await;

        // Listen to client messages and pass them to the room (not broadcasted)
        tokio::spawn(async move {
            let mut client_receiver = broadcast.subscribe();
            
            while let Ok(msg) = client_receiver.recv().await {
                if let Err(err) = room_tx.send(msg) {
                    println!("Failed forwarding message from target channel to room (?): {err}");
                }
            } 
        });

        // Listen to room messages and pass them to client
        tokio::spawn(async move {    
            while let Ok(msg) = room_rx.recv().await {
                println!("Broadcasting Message: {msg}");
                if let Err(err) = reply.send(msg) {
                    println!("Failed forwarding message from room to target channel {err}");
                }
            }
        });

        Ok(response_offer)
    }
}
