mod models;

use std::time::Duration;

use clap::{Parser, Subcommand};
use models::ClientRoomType;
use shared::api::APIClient;
use shared::connections::SteckerWebRTCConnection;
use shared::models::{ChannelName, PublicRoomType, RoomType, SteckerSendable};

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
            room_type,
            host,
        }) => {
            let _ = match room_type {
                ClientRoomType::Float => {
                    create_room::<f32>(name, host, room_type.clone(), 42.0).await
                }
                ClientRoomType::Chat => {
                    create_room::<String>(
                        name,
                        host,
                        room_type.clone(),
                        "some chat message".to_string(),
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
                ClientRoomType::Chat => {
                    let _ = join_room::<String>(name, host, room_type.clone());
                }
                ClientRoomType::Float => {
                    let _ = join_room::<f32>(name, host, room_type.clone());
                }
            };
        }
        None => {}
    }
}

async fn create_room<T: SteckerSendable>(
    name: &str,
    host: &str,
    client_room_type: ClientRoomType,
    value: T,
) -> anyhow::Result<()> {
    let connection = SteckerWebRTCConnection::build_connection().await?;

    let public_room_type: PublicRoomType = client_room_type.clone().into();
    let default_channel_name = ChannelName::from(RoomType::from(public_room_type.into()));

    let stecker_data_channel = connection
        .create_data_channel::<T>(&default_channel_name)
        .await?;

    let offer = connection.create_offer().await?;

    let api_client = APIClient {
        host: host.to_string(),
    };

    match api_client
        .create_room(name, &client_room_type.into(), &offer)
        .await
    {
        Ok(answer) => {
            // Apply the answer as the remote description
            connection.set_remote_description(answer).await?;

            println!("Press ctrl-c to stop");

            loop {
                let timeout = tokio::time::sleep(Duration::from_secs(5));
                tokio::pin!(timeout);

                tokio::select! {
                    _ = timeout.as_mut() =>{
                        // @todo check if the cloning can be reduced here?
                        println!("Send out {value}");
                        let _ = stecker_data_channel.outbound.send(value.clone());
                    },
                    _ = tokio::signal::ctrl_c() => {
                        println!("Pressed ctrl-c - shutting down");
                        break
                    }
                };
            }
            // should be a private method
            connection.close().await?;
            Ok(())
        }
        Err(err) => Err(err),
    }
}

async fn join_room<T: SteckerSendable>(
    name: &str,
    host: &str,
    client_room_type: ClientRoomType,
) -> anyhow::Result<()> {
    let connection = SteckerWebRTCConnection::build_connection().await?;

    let public_room_type: PublicRoomType = client_room_type.into();
    let default_channel_name = ChannelName::from(RoomType::from(public_room_type.clone().into()));

    let stecker_data_channel = connection
        .create_data_channel::<T>(&default_channel_name)
        .await?;
    let offer = connection.create_offer().await?;

    let api_client = APIClient {
        host: host.to_string(),
    };

    match api_client.join_room(name, &public_room_type, &offer).await {
        Ok(answer) => {
            // Apply the answer as the remote description
            connection.set_remote_description(answer).await?;

            println!("Press ctrl-c to stop");

            let mut receiver = stecker_data_channel.inbound.clone().subscribe();
            let mut close_receiver = stecker_data_channel.close_trigger.clone().subscribe();

            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Ok(m) => println!("received a message: {m}"),
                            Err(err) => {
                                println!("Error while receiving message - stop consuming messages: {err}");
                                break
                            },
                        }
                    }
                    _ = close_receiver.recv() => {
                        println!("received close signal!");
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
        Err(err) => Err(err),
    }
}
