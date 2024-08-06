pub mod models;
pub mod schema;
pub mod state;

use std::path::PathBuf;

use crate::schema::{Mutation, Query};

use async_graphql::{http::GraphiQLSource, EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    response::{self, IntoResponse},
    routing::get,
    Router,
};
use clap::Parser;
use state::AppState;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

const LOCAL_HOST: &str = "127.0.0.1";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// interface to listen on
    #[arg(long, default_value_t=LOCAL_HOST.to_string())]
    host: String,

    /// interface to listen on
    #[arg(long, default_value_t = 8000)]
    port: u16,
}

async fn graphiql() -> impl IntoResponse {
    response::Html(GraphiQLSource::build().endpoint("/").finish())
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let schema = Schema::build(Query, Mutation, EmptySubscription)
        .data(AppState::new())
        .finish();

    let app = Router::new()
        .route("/", get(graphiql).post_service(GraphQL::new(schema)))
        .nest_service(
            "/debug/",
            ServeDir::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates")),
        );

    println!("Start serving on http://{}:{}", args.host, args.port);
    axum::serve(
        TcpListener::bind((args.host, args.port)).await.unwrap(),
        app,
    )
    .await
    .unwrap();
}
