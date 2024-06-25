use std::{env, sync::Arc, time::Duration};

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use shared::utils::{decode_b64, encode_offer};
use tokio::sync::mpsc::Sender;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder,
    },
    data_channel::data_channel_message::DataChannelMessage,
    ice_transport::ice_server::RTCIceServer,
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, math_rand_alpha,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
};

const HOST: &str = "http://127.0.0.1:8000";

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    println!("{:?}", args);
    match args.get(1) {
        Some(uuid) => match run_to_join(uuid).await {
            Ok(_) => println!("this was the client that joind {uuid}"),
            Err(err) => println!("ERR running client: {err}"),
        },
        None => match run_to_create().await {
            Ok(_) => {
                println!("this was the client")
            }
            Err(err) => {
                println!("ERR running client: {err}")
            }
        },
    }
}
async fn create_connection(done_tx: Sender<()>) -> anyhow::Result<RTCPeerConnection> {
    // Create a MediaEngine object to configure the supported codec
    let mut m = MediaEngine::default();

    // Register default codecs
    m.register_default_codecs()?;

    // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
    // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
    // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
    // for each PeerConnection.
    let mut registry = Registry::new();

    // Use the default set of Interceptors
    registry = register_default_interceptors(registry, &mut m)?;

    // Create the API object with the MediaEngine
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Prepare the configuration
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    // Create a new RTCPeerConnection
    let peer_connection = api.new_peer_connection(config).await?;

    // Create a datachannel with label 'data'
    let data_channel = peer_connection.create_data_channel("data", None).await?;

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        println!("Peer Connection State has changed: {s}");

        if s == RTCPeerConnectionState::Failed {
            // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
            // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
            // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
            println!("Peer Connection has gone to failed exiting");
            let _ = done_tx.try_send(());
        }

        Box::pin(async {})
    }));

    // Register channel opening handling
    let d1 = Arc::clone(&data_channel);
    data_channel.on_open(Box::new(move || {
                  println!("Data channel '{}'-'{}' open. Random messages will now be sent to any connected DataChannels every 5 seconds", d1.label(), d1.id());

                  let d2 = Arc::clone(&d1);
                  Box::pin(async move {
                      let mut result = Result::<usize>::Ok(0);
                      while result.is_ok() {
                          let timeout = tokio::time::sleep(Duration::from_secs(5));
                          tokio::pin!(timeout);

                          tokio::select! {
                              _ = timeout.as_mut() =>{
                                  let message = math_rand_alpha(15);
                                  println!("Sending '{message}'");
                                  result = d2.send_text(message).await.map_err(Into::into);
                              }
                          };
                      }
                  })
              }));

    // Register text message handling
    let d_label = data_channel.label().to_owned();
    data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
        let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
        println!("Message from DataChannel '{d_label}': '{msg_str}'");
        Box::pin(async {})
    }));

    Ok(peer_connection)
}

async fn create_rtc_offer(peer_connection: &RTCPeerConnection) -> anyhow::Result<String> {
    // Create an offer to send to the browser
    let offer = peer_connection.create_offer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(offer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can safely transfer it as a json value
    if let Some(local_desc) = peer_connection.local_description().await {
        let b64 = encode_offer(local_desc)?;
        Ok(b64)
    } else {
        println!("generate local_description failed!");
        Err(anyhow::Error::msg("generate local_description failed!"))
    }
}

async fn run_to_create() -> anyhow::Result<()> {
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

    let peer_connection = create_connection(done_tx).await?;
    let offer = create_rtc_offer(&peer_connection).await?;

    match create_room("foo", &offer).await {
        Ok(answer) => {
            // Apply the answer as the remote description
            peer_connection.set_remote_description(answer).await?;

            println!("Press ctrl-c to stop");
            tokio::select! {
                _ = done_rx.recv() => {
                    println!("received done signal!");
                }
                _ = tokio::signal::ctrl_c() => {
                    println!();
                }
            };

            peer_connection.close().await?;
            Ok(())
        }
        Err(err) => Err(err),
    }
}

async fn run_to_join(uuid: &str) -> anyhow::Result<()> {
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

    let peer_connection = create_connection(done_tx).await?;
    let offer = create_rtc_offer(&peer_connection).await?;

    match join_room(uuid, &offer).await {
        Ok(answer) => {
            // Apply the answer as the remote description
            peer_connection.set_remote_description(answer).await?;

            println!("Press ctrl-c to stop");
            tokio::select! {
                _ = done_rx.recv() => {
                    println!("received done signal!");
                }
                _ = tokio::signal::ctrl_c() => {
                    println!();
                }
            };

            peer_connection.close().await?;
            Ok(())
        }
        Err(err) => Err(err),
    }
}

// API Calls

async fn create_room(
    name: &str,
    local_session_description: &str,
) -> anyhow::Result<RTCSessionDescription> {
    let query = json!({
        "query": "mutation createRoom($name:String!, $offer:String!) { createRoom(name:$name, offer:$offer) }",
        "variables": {
            "name": name,
            "offer": local_session_description,
        }
    });
    let client = reqwest::Client::new();
    let res = client
        .post(HOST)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(query.to_string())
        .send()
        .await?;
    let results = res.json::<GQLResponse<CreateRoomData>>().await?;
    println!("Response from server: {results:?}");
    let desc_data = decode_b64(results.data.create_room.as_str())?;
    let answer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

    Ok(answer)
}

async fn join_room(
    uuid: &str,
    local_session_description: &str,
) -> anyhow::Result<RTCSessionDescription> {
    let query = json!({
        "query": "mutation joinRoom($uuid:String!, $offer:String!) { joinRoom(roomUuid:$uuid, offer:$offer) }",
        "variables": {
            "uuid": uuid,
            "offer": local_session_description,
        }
    });
    let client = reqwest::Client::new();
    let res = client
        .post(HOST)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(query.to_string())
        .send()
        .await?;
    println!("{res:?}");
    match res.json::<GQLResponse<JoinRoomData>>().await {
        Ok(results) => {
            let desc_data = decode_b64(results.data.join_room.as_str())?;
            let answer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

            Ok(answer)
        }
        Err(err) => {
            println!("ERR joinRoom response: {err}");
            Err(err.into())
        }
    }
}

#[derive(Deserialize, Debug)]
struct GQLResponse<T> {
    data: T
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CreateRoomData {
    create_room: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JoinRoomData {
    join_room: String,
}
