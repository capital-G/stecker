pub mod models;
pub mod schema;
pub mod state;

use std::path::PathBuf;

use crate::schema::{Mutation, Query};

use async_graphql::extensions::Tracing;
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
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{self, filter};

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
    response::Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let filter = filter::Targets::new()
        .with_default(Level::ERROR)
        .with_target("server", Level::DEBUG);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let schema = Schema::build(Query, Mutation, EmptySubscription)
        .data(AppState::new())
        .extension(Tracing)
        .finish();

    let app = Router::new()
        .route("/graphql", get(graphiql).post_service(GraphQL::new(schema)))
        .nest_service(
            "/",
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
