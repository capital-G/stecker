use crate::{models::SteckerData, utils::decode_b64};
use anyhow::bail;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use tracing::{error, instrument, trace};
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[derive(Deserialize, Debug)]
struct GQLResponse<T> {
    data: T,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CreateRoomDataRaw {
    offer: String,
    password: String,
}

// @todo remove this unecessary reply wrapper...
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CreateRoomWrapper {
    pub create_room: CreateRoomDataRaw,
}

pub struct CreateRoomResponse {
    pub session_description: RTCSessionDescription,
    pub password: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JoinRoomData {
    join_room: String,
}

pub struct APIClient {
    graphql_url: String,
}

impl APIClient {
    pub fn new(host: String) -> Self {
        Self {
            graphql_url: format!("{host}/graphql"),
        }
    }
}

impl APIClient {
    #[instrument(skip_all, err)]
    pub async fn create_room<T: SteckerData>(
        &self,
        name: &str,
        password: Option<&str>,
        local_session_description: &str,
    ) -> anyhow::Result<CreateRoomResponse> {
        let room_string: String = T::label();

        // @todo skip serialization of password if none, see https://serde.rs/field-attrs.html#skip_serializing_if
        let used_password = password.unwrap_or("");

        let query = json!({
            "query": r#"
                mutation createRoom($name: String!, $offer: String!, $channelType: ChannelType!, $password: String) {
                  createRoom(
                    name: $name
                    offer: $offer
                    channelType: $channelType
                    password: $password
                  ) {
                    password
                    offer
                  }
                }
            "#,
            "variables": {
                "name": name,
                "offer": local_session_description,
                "channelType": T::label(),
                "password": used_password,
            }
        });
        println!("JSON: {}", query);

        let client = reqwest::Client::new();
        let res = client
            .post(&self.graphql_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(query.to_string())
            .send()
            .await?;

        let text = res.text().await?;
        let json: anyhow::Result<GQLResponse<CreateRoomWrapper>> =
            serde_json::from_str::<GQLResponse<CreateRoomWrapper>>(&text).map_err(Into::into);

        match json {
            Ok(result) => {
                trace!(
                    result.data.create_room.password,
                    result.data.create_room.offer,
                    "Response from server"
                );
                let desc_data = decode_b64(result.data.create_room.offer.as_str())?;
                let answer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
                return Ok(CreateRoomResponse {
                    session_description: answer,
                    password: result.data.create_room.password,
                });
            }
            Err(err) => {
                bail!("Received unexpected response from server: {err} - {text}");
            }
        }
    }

    #[instrument(skip_all, err)]
    pub async fn join_room<T: SteckerData>(
        &self,
        name: &str,
        local_session_description: &str,
    ) -> anyhow::Result<RTCSessionDescription> {
        let room_string = T::label();
        let query = json!({
            "query": r#"mutation joinRoom($name: String!, $offer: String!, $channelType: ChannelType!) {
                joinRoom(name: $name, offer: $offer, channelType: $channelType)
            }"#,
            "variables": {
                "name": name,
                "offer": local_session_description,
                "channelType": room_string,
            }
        });

        let client = reqwest::Client::new();
        let res = client
            .post(&self.graphql_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(query.to_string())
            .send()
            .await?;
        let status_code = res.status();
        let text = res.text().await?;

        if status_code != StatusCode::OK {
            bail!("Join room request failed (status code {status_code}): Response was {text}");
        }

        match serde_json::from_str::<GQLResponse<JoinRoomData>>(&text) {
            Ok(results) => {
                let desc_data = decode_b64(results.data.join_room.as_str())?;
                let answer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
                Ok(answer)
            }
            Err(err) => {
                error!(response = text, "Failed to join room");
                Err(err.into())
            }
        }
    }
}
