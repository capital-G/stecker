mod models;

use std::time::Duration;

use clap::{Parser, Subcommand};
use models::ClientRoomType;
use shared::api::APIClient;
use shared::connections::SteckerWebRTCConnection;
use shared::models::{DataRoomType, PublicRoomType, SteckerData};

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
                    create_room(name, host, room_type.clone(), SteckerData::F32(42.0)).await
                }
                ClientRoomType::Chat => {
                    create_room(
                        name,
                        host,
                        room_type.clone(),
                        SteckerData::String("Hello?".to_string()),
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
                    let _ = join_room(name, host, room_type).await;
                }
                ClientRoomType::Float => {
                    let _ = join_room(name, host, room_type).await;
                }
            };
        }
        None => {}
    }
}

async fn create_room(
    name: &str,
    host: &str,
    client_room_type: ClientRoomType,
    value: SteckerData,
) -> anyhow::Result<()> {
    let connection = SteckerWebRTCConnection::build_connection().await?;

    let public_room_type = PublicRoomType::from(client_room_type.clone());
    let room_type = DataRoomType::from(client_room_type.clone());

    let meta_data_channel = connection.create_data_channel(&DataRoomType::Meta).await?;
    let mut meta_msg_receiver = meta_data_channel.inbound.subscribe();

    let data_channel = connection.create_data_channel(&room_type).await?;
    let data_outbound = data_channel.outbound.clone();

    let api_client = APIClient {
        host: host.to_string(),
    };

    let offer = connection.create_offer().await?;

    match api_client
        .create_room(name, &public_room_type, &offer)
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
                        println!("Send value: {value}");
                        let _ = data_outbound.send(value.clone());
                    },
                    raw_meta_msg = meta_msg_receiver.recv() => {
                        if let Ok(SteckerData::String(msg)) = raw_meta_msg {
                            println!("META: {msg}");
                        } else {
                            println!("Error while receiving meta message");
                        }
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
            println!("Could not create a room via the API: {err}");
            Err(err)
        }
    }
}

async fn join_room(
    name: &str,
    host: &str,
    client_room_type: &ClientRoomType,
) -> anyhow::Result<()> {
    let public_room_type = PublicRoomType::from(client_room_type.clone());
    let room_type = DataRoomType::from(client_room_type.clone());

    let connection = SteckerWebRTCConnection::build_connection().await?;

    let stecker_data_channel = connection.create_data_channel(&room_type).await?;
    let stecker_meta_channel = connection.create_data_channel(&DataRoomType::Meta).await?;

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
            let mut meta_receiver = stecker_meta_channel.inbound.clone().subscribe();
            let mut close_receiver = stecker_data_channel.close.clone().subscribe();

            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Ok(stecker_data) => {
                                println!("Received {stecker_data}");
                            },
                            Err(err) => {
                                println!("Error while receiving message - stop consuming messages: {err}");
                                break
                            },
                        }
                    }
                    raw_meta_msg = meta_receiver.recv() => {
                        if let Ok(SteckerData::String(msg)) = raw_meta_msg {
                            println!("META: {msg}");
                        } else {
                            println!("Error while receiving meta message");
                        }
                    },
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
        Err(err) => {
            println!("Wrong server reply: {err}");
            Err(err)
        }
    }
}
