mod models;

use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use models::ClientRoomType;
use shared::api::APIClient;
use shared::connections::{ConnectionEvent, SteckerWebRTCConnection};
use shared::models::{
    RoomFloatData, RoomStringData, SteckerData, SteckerDataChanelTrait, SteckerDataChannel,
};
use tokio::sync::broadcast;

const LOCAL_HOST: &str = "http://127.0.0.1:8000";

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

        /// admin password of the room
        password: Option<String>,

        /// type of room
        #[clap(value_enum, default_value_t=ClientRoomType::Float)]
        room_type: ClientRoomType,

        /// address of the stecker server
        #[arg(long, default_value_t=LOCAL_HOST.to_string())]
        host: String,
    },
    /// join an existing broadcast room
    JoinRoom {
        /// name of the room to join
        name: String,

        /// type of room
        #[clap(value_enum, default_value_t=ClientRoomType::Float)]
        room_type: ClientRoomType,

        /// address of the stecker server
        #[arg(long, default_value_t=LOCAL_HOST.to_string())]
        host: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::CreateRoom {
            name,
            password,
            room_type,
            host,
        }) => {
            let _ = match room_type {
                ClientRoomType::Float => {
                    create_room::<RoomFloatData>(name, password.as_deref(), host, 42.0).await
                }
                ClientRoomType::Chat => {
                    create_room::<RoomStringData>(
                        name,
                        password.as_deref(),
                        host,
                        "Hello?".to_string(),
                    )
                    .await
                }
            };
        }
        Some(Commands::JoinRoom {
            name,
            room_type,
            host,
        }) => {
            let _ = match room_type {
                ClientRoomType::Float => join_room::<RoomFloatData>(name, host).await,
                ClientRoomType::Chat => join_room::<RoomStringData>(name, host).await,
            };
        }
        None => {}
    }
}

async fn create_room<T>(
    name: &str,
    password: Option<&str>,
    host: &str,
    value: T::Payload,
) -> anyhow::Result<()>
where
    T: SteckerData,
    T::Payload: Display + Clone,
    SteckerDataChannel<T>: SteckerDataChanelTrait,
{
    let (events, _) = broadcast::channel::<ConnectionEvent>(16);
    let connection = SteckerWebRTCConnection::build_connection(events).await?;

    let data_channel = Arc::new(SteckerDataChannel::<T>::create_channels());
    let data_outbound = data_channel.outbound.clone();

    connection.create_data_channel(data_channel).await?;

    let offer = connection.create_offer().await?;
    let api_client = APIClient::new(host.to_string());

    match api_client.create_room::<T>(name, password, &offer).await {
        Ok(answer) => {
            connection
                .set_remote_description(answer.session_description)
                .await?;

            println!("Press ctrl-c to stop");

            loop {
                let timeout = tokio::time::sleep(Duration::from_secs(5));
                tokio::pin!(timeout);

                tokio::select! {
                    _ = timeout.as_mut() => {
                        println!("Send value: {value}");
                        let _ = data_outbound.send(value.clone());
                    },
                    _ = tokio::signal::ctrl_c() => {
                        println!("Pressed ctrl-c - shutting down");
                        break
                    }
                };
            }
            connection.close().await?;
            Ok(())
        }
        Err(err) => {
            println!("Could not create a room via the API: {err}");
            Err(err)
        }
    }
}

async fn join_room<T>(name: &str, host: &str) -> anyhow::Result<()>
where
    T: SteckerData,
    T::Payload: Display,
    SteckerDataChannel<T>: SteckerDataChanelTrait,
{
    let (events, _) = broadcast::channel::<ConnectionEvent>(16);
    let connection = SteckerWebRTCConnection::build_connection(events).await?;

    let data_channel = Arc::new(SteckerDataChannel::<T>::create_channels());
    let mut inbound = data_channel.inbound.subscribe();

    connection.create_data_channel(data_channel).await?;

    let offer = connection.create_offer().await?;
    let api_client = APIClient::new(host.to_string());

    match api_client.join_room::<T>(name, &offer).await {
        Ok(answer) => {
            connection.set_remote_description(answer).await?;

            println!("Press ctrl-c to stop");

            loop {
                tokio::select! {
                    msg = inbound.recv() => {
                        match msg {
                            Ok(data) => {
                                println!("Received {data}");
                            },
                            Err(err) => {
                                println!("Error while receiving message: {err}");
                                break
                            },
                        }
                    }
                    _ = connection.wait_for_disconnect() => {
                        println!("Server closed connection");
                        break
                    }
                    _ = tokio::signal::ctrl_c() => {
                        println!("Pressed ctrl-c - shutting down");
                        break
                    }
                };
            }

            connection.close().await?;
            Ok(())
        }
        Err(err) => {
            println!("Wrong server reply: {err}");
            Err(err)
        }
    }
}
