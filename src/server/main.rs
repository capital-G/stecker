pub mod models;
pub mod osc_listener;
pub mod schema;
pub mod state;
pub mod views;

use std::sync::Arc;

use crate::schema::{Mutation, Query};
use crate::views::DebugTemplate;

use async_graphql::extensions::Tracing;
use async_graphql::{http::GraphiQLSource, EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    response::{self, IntoResponse},
    routing::get,
    Router,
};
use clap::Parser;
use osc_listener::handle_osc_client;
use state::AppState;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing::{debug, error, info, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{self, filter};
use views::{dispatcher_view, stream_view};

const LOCAL_HOST: &str = "127.0.0.1";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// interface to listen on
    #[arg(long, default_value_t=LOCAL_HOST.to_string())]
    host: String,

    /// http port to listen on
    #[arg(long, default_value_t = 8000)]
    port: u16,

    /// tcp osc port to listen on
    #[arg(long, default_value_t = 8001)]
    osc_port: u16,
}

async fn graphiql() -> impl IntoResponse {
    response::Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let filter = filter::Targets::new()
        .with_default(Level::ERROR)
        .with_target("server", Level::TRACE)
        .with_target("shared", Level::TRACE);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let state = Arc::new(AppState::new());

    let schema = Schema::build(Query, Mutation, EmptySubscription)
        .data(state.clone())
        .extension(Tracing)
        .finish();

    let host2 = args.host.clone();

    let http_app = Router::new()
        .route("/graphql", get(graphiql).post_service(GraphQL::new(schema)))
        .nest_service(
            "/s/:name",
            ServeFile::new(templates_dir.join("stream.html")),
        )
        .nest_service(
            "/d/:name",
            ServeFile::new(templates_dir.join("dispatcher.html")),
        )
        .nest_service("/", ServeDir::new(templates_dir));

    let http_handle = tokio::spawn(async move {
        info!("Start http serving on http://{}:{}", args.host, args.port);
        axum::serve(
            TcpListener::bind((args.host, args.port)).await.unwrap(),
            http_app,
        )
        .await
    });

    let osc_app = tokio::spawn(async move {
        info!("Start TCP-OSC serving on {}:{}", host2, args.osc_port);
        let tcp_osc_listener = tokio::net::TcpListener::bind((host2, args.osc_port))
            .await
            .unwrap();
        loop {
            match tcp_osc_listener.accept().await {
                Ok((osc_socket, addr)) => {
                    debug!("New OSC connection from {:?}:{:?}", addr.ip(), addr.port());
                    tokio::spawn(async move { handle_osc_client(osc_socket, addr).await });
                }
                Err(e) => error!("TCP-OSC connection failed: {:?}", e),
            }
        }
    });

    let _ = tokio::join!(http_handle, osc_app,);
}
