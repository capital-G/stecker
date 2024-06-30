use std::time::Duration;

mod api;

use api::APIClient;
use clap::{Parser, Subcommand};
use shared::connections::SteckerWebRTCConnection;
use webrtc::peer_connection::math_rand_alpha;

const HOST: &str = "http://127.0.0.1:8000";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// create a new broadcast room
    CreateRoom {
        /// name of the new room
        name: String,
    },
    /// join an existing broadcast room
    JoinRoom {
        /// uuid of the room to join
        uuid: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::CreateRoom { name }) => {
            let _ = create_room(name).await;
        }
        Some(Commands::JoinRoom { uuid }) => {
            let _ = join_room(uuid).await;
        }
        None => {}
    }
}

async fn create_room(name: &str) -> anyhow::Result<()> {
    let (_, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

    let connection = SteckerWebRTCConnection::build_connection().await?;
    let stecker_data_channel = connection.create_data_channel("foo").await?;
    let offer = connection.create_offer().await?;

    tokio::spawn(async move {
        // todo make this stop on connection close
        loop {
            let timeout = tokio::time::sleep(Duration::from_secs(5));
            tokio::pin!(timeout);

            tokio::select! {
                _ = timeout.as_mut() =>{
                    let message = math_rand_alpha(15);
                    println!("Sending '{message}'");
                    let _ = stecker_data_channel.outbound.send(message);
                }
            };
        }
    });

    let api_client = APIClient {
        host: HOST.to_string(),
    };

    match api_client.create_room(name, &offer).await {
        Ok(answer) => {
            // Apply the answer as the remote description
            connection.set_remote_description(answer).await?;

            println!("Press ctrl-c to stop");
            tokio::select! {
                _ = done_rx.recv() => {
                    println!("received done signal!");
                }
                _ = tokio::signal::ctrl_c() => {
                    println!();
                }
            };

            // should be a private method
            connection.close().await?;
            Ok(())
        }
        Err(err) => Err(err),
    }
}

async fn join_room(uuid: &str) -> anyhow::Result<()> {
    let connection = SteckerWebRTCConnection::build_connection().await?;
    let stecker_data_channel = connection.create_data_channel("foo").await?;
    let offer = connection.create_offer().await?;

    let api_client = APIClient {
        host: HOST.to_string(),
    };

    match api_client.join_room(uuid, &offer).await {
        Ok(answer) => {
            // Apply the answer as the remote description
            connection.set_remote_description(answer).await?;

            let mut receiver = stecker_data_channel.inbound.clone().subscribe();

            tokio::spawn(async move {
                loop {
                    let msg = receiver.recv().await;
                    match msg {
                        Ok(m) => println!("received a message: {m}"),
                        Err(_) => println!("Error while receiving message"),
                    }
                }
            });

            println!("Press ctrl-c to stop");
            let mut close_receiver = stecker_data_channel.close_trigger.clone().subscribe();
            tokio::select! {
                _ = close_receiver.recv() => {
                    println!("received close signal!");
                }
                _ = tokio::signal::ctrl_c() => {
                    println!();
                }
            };

            connection.close().await?;
            Ok(())
        }
        Err(err) => Err(err),
    }
}
