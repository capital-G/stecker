pub mod broadcast_room;
pub mod schema;

use std::{collections::HashMap, sync::Arc};

use crate::broadcast_room::BroadcastRoom;
use crate::schema::{Mutation, Query};

use async_graphql::{http::GraphiQLSource, EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    response::{self, IntoResponse},
    routing::get,
    Router,
};
// use models::BroadcastRoom;
use tokio::{net::TcpListener, sync::Mutex};
use tower_http::services::ServeDir;

struct AppState {
    pub rooms: Mutex<HashMap<String, Arc<Mutex<BroadcastRoom>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            rooms: Mutex::new(HashMap::new()),
        }
    }
}

async fn graphiql() -> impl IntoResponse {
    response::Html(GraphiQLSource::build().endpoint("/").finish())
}

#[tokio::main]
async fn main() {
    let schema = Schema::build(Query, Mutation, EmptySubscription)
        .data(AppState::new())
        .finish();

    let app = Router::new()
        .route("/", get(graphiql).post_service(GraphQL::new(schema)))
        .nest_service("/debug", ServeDir::new("./src/server/templates"));

    println!("GraphiQL IDE: http://127.0.0.1:8000");

    axum::serve(TcpListener::bind("0.0.0.0:8000").await.unwrap(), app)
        .await
        .unwrap();
}
