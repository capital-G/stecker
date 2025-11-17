use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect},
};

use crate::state::{AppState, RoomMapTrait};

pub enum Template {
    Debug,
    Stream,
    DispatcherNotFound,
    DispatcherNoRoomAvailable,
}

impl Template {
    fn as_str(&self) -> &'static str {
        match self {
            Template::Debug => "debug.html.jinja",
            Template::Stream => "stream.html.jinja",
            Template::DispatcherNotFound => "dispatcher_not_found.html.jinja",
            Template::DispatcherNoRoomAvailable => "dispatcher_no_room_available.html.jinja",
        }
    }
}

pub async fn debug_view(State(state): State<Arc<AppState>>) -> Html<String> {
    let template = state
        .jinja
        .get_template(Template::Debug.as_str())
        .expect("Missing debug template");
    let rendered = template
        .render(minijinja::context! {})
        .expect("failed to render debug template");

    Html(rendered)
}

pub async fn stream_view(
    State(state): State<Arc<AppState>>,
    Path(room_name): Path<String>,
) -> Html<String> {
    let room_guard = state.audio_rooms.map.read().await;
    let room_value = room_guard.get(&room_name);

    let room_name = match room_value {
        Some(room) => {
            let guard = room.read().await;
            Some(guard.meta().name.clone())
        }
        None => None,
    };

    let template = state
        .jinja
        .get_template(Template::Stream.as_str())
        .expect("Stream template not found!");
    let rendered = template
        .render(minijinja::context! {
            room_name => room_name,
        })
        .expect("Rendering of stream view failed");

    Html(rendered)
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
                        let uri = match dispatcher.return_room_prefix.clone() {
                            Some(return_roof_prefix) => {
                                format!("/s/{}?returnRoomPrefix={}", room.name, return_roof_prefix)
                            }
                            None => {
                                format!("/s/{}", room.name)
                            }
                        };
                        Ok(Redirect::to(&uri.as_str()).into_response())
                    }
                    Err(_) => {
                        let template = state
                            .jinja
                            .get_template(Template::DispatcherNoRoomAvailable.as_str())
                            .expect("Could not find dispatcher no room available template");
                        let rendered = template
                            .render(minijinja::context! {})
                            .expect("Failed to render dispatcher no room available template");
                        Ok(Html(rendered).into_response())
                    }
                }
            }
        }
    } else {
        let template = state
            .jinja
            .get_template(Template::DispatcherNotFound.as_str())
            .expect("Could not find dispatcher not found template");
        let rendered = template
            .render(minijinja::context! {})
            .expect("Failed to render dispatcher not found template");
        Ok(Html(rendered).into_response())
    }
}
