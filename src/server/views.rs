use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
};

use crate::state::AppState;

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
    let room_name = {
        let rooms_guard = state.rooms.read().await;
        rooms_guard.get(&room_name).map(|_| room_name.clone())
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
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    if let Some(dispatcher) = state.room_dispatchers.read().await.get(&dispatcher_name) {
        match state.get_room(dispatcher).await {
            Ok(room) => {
                let room = room.read().await;
                let mut uri = format!("/s/{}?", room.meta().name);
                if let Some(return_prefix) = dispatcher.return_room_prefix.clone() {
                    uri.push_str(format!("&returnRoomPrefix={}", return_prefix).as_str());
                }
                if dispatcher.add_random_postfix {
                    uri.push_str("&addRandomPostfix=1");
                }
                Ok(Redirect::to(uri.as_str()).into_response())
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
