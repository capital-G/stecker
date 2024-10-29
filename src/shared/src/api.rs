use crate::{
    models::{DataRoomPublicType, SteckerAPIRoomType},
    utils::decode_b64,
};
use anyhow::bail;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info, instrument};
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[derive(Deserialize, Debug)]
struct GQLResponse<T> {
    data: T,
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

// @todo make this static?
impl Into<String> for &SteckerAPIRoomType {
    fn into(self) -> String {
        match self {
            SteckerAPIRoomType::Audio => "AUDIO".to_owned(),
            SteckerAPIRoomType::Data(data_channel) => match data_channel {
                DataRoomPublicType::Float => "FLOAT".to_owned(),
                DataRoomPublicType::Chat => "CHAT".to_owned(),
            },
        }
    }
}

impl APIClient {
    #[instrument(skip_all, err)]
    pub async fn create_room(
        &self,
        name: &str,
        room_type: &SteckerAPIRoomType,
        local_session_description: &str,
    ) -> anyhow::Result<RTCSessionDescription> {
        let room_string: String = room_type.into();
        let query = json!({
            "query": "mutation createRoom($name:String!, $offer:String!, $roomType: RoomType!) { createRoom(name:$name, offer:$offer, roomType:$roomType) }",
            "variables": {
                "name": name,
                "offer": local_session_description,
                "roomType": room_string,
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

        let text = res.text().await?;
        let json: anyhow::Result<GQLResponse<CreateRoomData>> =
            serde_json::from_str::<GQLResponse<CreateRoomData>>(&text).map_err(Into::into);

        match json {
            Ok(result) => {
                info!(result.data.create_room, "Response from server");
                let desc_data = decode_b64(result.data.create_room.as_str())?;
                let answer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
                return Ok(answer);
            }
            Err(err) => {
                bail!("Received unexpected response from server: {err} - {text}");
            }
        }
    }

    #[instrument(skip_all, err)]
    pub async fn join_room(
        &self,
        name: &str,
        room_type: &SteckerAPIRoomType,
        local_session_description: &str,
    ) -> anyhow::Result<RTCSessionDescription> {
        let room_string: String = room_type.into();
        let query = json!({
            "query": "mutation joinRoom($name:String!, $offer:String!, $roomType:RoomType!) { joinRoom(name:$name, offer:$offer, roomType:$roomType) }",
            "variables": {
                "name": name,
                "offer": local_session_description,
                "roomType": room_string,
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
