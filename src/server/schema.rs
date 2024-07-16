use crate::broadcast_room::{BroadcastRoom, RoomType};
use shared::connections::SteckerWebRTCConnection;

use std::{collections::HashMap, sync::Arc};
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
    room_type: RoomType,
}

#[Object]
impl Query {
    async fn rooms<'a>(&self, ctx: &Context<'a>, room_type: Option<RoomType>) -> Result<Vec<Room>> {
        let state = ctx.data_unchecked::<AppState>();
        let rooms = state.rooms.lock().await;

        // map does not support async which is necessary due to async,
        // therefore we use this atrocity
        let mut results = Vec::with_capacity(rooms.len());
        for (name, room) in rooms.iter() {
            let locked_room = room.lock().await;
            match room_type {
                Some(rt) => {
                    if locked_room.room_type == rt {
                        results.push(Room {
                            uuid: locked_room.uuid.clone().into(),
                            name: name.clone(),
                            num_listeners: 0,
                            room_type: rt,
                        });
                    }
                }
                None => {
                    results.push(Room {
                        uuid: locked_room.uuid.clone().into(),
                        name: name.clone(),
                        num_listeners: 0,
                        room_type: RoomType::Float,
                    });
                }
            }
        }
        Ok(results)
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn reset_rooms<'a>(&self, ctx: &Context<'a>) -> f32 {
        let mut rooms = ctx.data_unchecked::<AppState>().rooms.lock().await;
        *rooms = HashMap::new();
        0.
    }

    async fn create_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();

        {
            // create a block b/c otherwise we run into deadlock with mutex
            let mut rooms = state.rooms.lock().await;
            let room = rooms.get_mut(&name);

            if room.is_some() {
                return Err("Room with name {name} already exists.".into());
            }
        }

        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer).await?;

        let stecker_data_channel = connection.listen_for_data_channel::<f32>().await;

        let room = BroadcastRoom {
            name: name.clone(),
            reply: stecker_data_channel.outbound.clone(),
            broadcast: stecker_data_channel.inbound,
            uuid: Uuid::new_v4(),
            room_type: RoomType::Float,
        };

        {
            let mut rooms = state.rooms.lock().await;
            let room_mutex = Arc::new(AsyncMutex::new(room));
            rooms.insert(name.clone(), room_mutex.clone());
        }

        return Ok(response_offer);
    }

    async fn join_room<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
        offer: String,
    ) -> Result<String> {
        let state = ctx.data_unchecked::<AppState>();
        let mut rooms = state.rooms.lock().await;
        let room = rooms.get_mut(&name);

        if room.is_none() {
            return Err(format!("No such room {name}").into());
        }

        let connection = SteckerWebRTCConnection::build_connection().await?;
        let response_offer = connection.respond_to_offer(offer).await?;
        let stecker_data_channel = connection.listen_for_data_channel::<f32>().await;

        let room = room.unwrap().lock().await;

        let room_rx = room.broadcast.clone();
        // let room_tx = room.reply.clone();
        let close_trigger2 = stecker_data_channel.close_trigger.clone();

        // Listen to client messages and pass them to the room (not broadcasted)
        tokio::spawn(async move {
            // let mut client_receiver = broadcast.subscribe();
            let mut room_receiver = room_rx.subscribe();
            let mut stop_receiver = stecker_data_channel.close_trigger.subscribe();
            let mut consume = true;

            while consume {
                tokio::select! {
                    raw_msg = room_receiver.recv() => {
                        match raw_msg {
                            Ok(msg) => {
                                let _ = stecker_data_channel.outbound.send(msg);
                            },
                            Err(err) => {
                                while room_receiver.len() > 0 {
                                    let _ = room_receiver.recv().await;
                                }
                                println!("Got some lagging problems: {err}");
                            },
                        }
                    },
                    _ = stop_receiver.recv() => {
                        println!("Received a stop signal");
                        consume = false;
                    }
                };
            }
        });

        // Listen to room messages and pass them to client
        let mut inbound_receiver = stecker_data_channel.inbound.subscribe();
        let mut stop_receiver2 = close_trigger2.subscribe();
        tokio::spawn(async move {
            let mut consume = true;
            while consume {
                tokio::select! {
                    raw_msg = inbound_receiver.recv() => {
                        match raw_msg {
                            Ok(msg) => {println!("Broadcasting message from subscriber - will be ignored {msg}");},
                            Err(err) => {println!("Got errors when receiving inbound message: {err}")},
                        }
                    },
                    _ = stop_receiver2.recv() => {
                        println!("Got triggered and stop consuming inbound messages now");
                        consume = false;
                    }
                }
            }
        });

        Ok(response_offer)
    }
}
