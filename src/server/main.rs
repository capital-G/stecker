pub mod models;
pub mod schema;
pub mod connections;

use std::collections::HashMap;

use async_graphql::{http::GraphiQLSource, EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    response::{self, IntoResponse},
    routing::get,
    Router,
};
use models::{BroadcastRoom, User};
use crate::schema::{Query, Mutation};
use tokio::{net::TcpListener, sync::Mutex};

struct AppState {
    pub counter: Mutex<i32>,
    pub rooms: Mutex<HashMap<String, BroadcastRoom>>
}

impl AppState {
    pub fn new() -> Self {
        Self {
            counter: Mutex::new(0),
            rooms: Mutex::new(HashMap::new())
            // rooms: Mutex::new( HashMap::from([
            //         (
            //             "a".to_owned(),
            //             Room{
            //                 name: "my room".into(),
            //                 listeners: vec![],
            //                 broadcaster: User { id: "foo".to_owned(), name: "bar".to_owned() }
            //             }
            //         ),
            //         (
            //             "b".to_owned(),
            //             Room{
            //                 name: "my other room".into(),
            //                 listeners: vec![],
            //                 broadcaster: User { id: "foobar".to_owned(), name: "baz".to_owned() }
            //             }
            //         ),
            // ]))
        }
    }
}

async fn graphiql() -> impl IntoResponse {
    response::Html(GraphiQLSource::build().endpoint("/").finish())
}

#[tokio::main]
async fn main() {
    // let shared_state = Arc::new(AppState { counter: 0 });

    let schema = Schema::build( Query, Mutation, EmptySubscription)
        // .data(StarWars::new())
        .data(AppState::new())
        .finish();

    let app = Router::new()
        // .with_state(shared_state)
        .route("/", get(graphiql)
        .post_service(GraphQL::new(schema)));

    println!("GraphiQL IDE: http://localhost:8000");

    axum::serve(TcpListener::bind("127.0.0.1:8000").await.unwrap(), app)
        .await
        .unwrap();
}
