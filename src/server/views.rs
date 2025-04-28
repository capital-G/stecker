use core::fmt;
use std::sync::Arc;

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use tracing::error;

use crate::state::{AppState, RoomMapTrait};

#[derive(Template, WebTemplate)]
#[template(path = "debug.html.jinja")]
pub struct DebugTemplate {}

#[derive(Clone, Debug)]
struct RoomInfo {
    name: String,
}

#[derive(Template, WebTemplate, Clone, Debug)]
#[template(path = "stream.html.jinja")]
pub struct StreamTemplate {
    room: Option<RoomInfo>,
}

pub async fn stream_view(
    State(state): State<Arc<AppState>>,
    Path(room_name): Path<String>,
) -> StreamTemplate {
    let map_guard = state.audio_rooms.map.read().await;
    let room_value = map_guard.get(&room_name);

    if let Some(room) = room_value {
        StreamTemplate {
            room: Some(RoomInfo {
                name: room.read().await.meta().name.to_owned(),
            }),
        }
    } else {
        StreamTemplate { room: None }
    }
}

#[derive(Template, WebTemplate, Clone, Debug)]
#[template(path = "dispatcher_not_found.html.jinja")]
pub struct DispatcherNotFoundTemplate {
    dispatcher_name: String,
}

#[derive(Template, WebTemplate, Clone, Debug)]
#[template(path = "dispatcher_no_room_available.html.jinja")]
pub struct DispatcherNoRoomAvailableTemplate {
    dispatcher_name: String,
}

pub async fn dispatcher_view(
    State(state): State<Arc<AppState>>,
    Path(dispatcher_name): Path<String>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    if let Some(dispatcher) = state.room_dispatchers.read().await.get(&dispatcher_name) {
        match dispatcher.room_type {
            crate::models::RoomType::Float => todo!(),
            crate::models::RoomType::Chat => todo!(),
            crate::models::RoomType::Audio => {
                let room_result = state.audio_rooms.get_room(dispatcher).await;
                match room_result {
                    Ok(room) => {
                        // @todo how to make this type safe?
                        Ok(Redirect::to(format!("/s/{}", room.name).as_str()).into_response())
                    }
                    Err(_) => {
                        Ok(DispatcherNoRoomAvailableTemplate { dispatcher_name }.into_response())
                    }
                }
            }
        }
    } else {
        Ok(DispatcherNotFoundTemplate { dispatcher_name }.into_response())
    }
}
