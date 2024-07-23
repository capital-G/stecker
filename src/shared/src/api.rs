use crate::{
    models::PublicRoomType,
    utils::decode_b64,
};
use anyhow::bail;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
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
    pub host: String,
}

// @todo make this static?
impl Into<String> for &PublicRoomType {
    fn into(self) -> String {
        match self {
            PublicRoomType::Float => "FLOAT".to_string(),
            PublicRoomType::Chat => "CHAT".to_string(),
        }
    }
}

impl APIClient {
    pub async fn create_room(
        &self,
        name: &str,
        room_type: &PublicRoomType,
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
            .post(&self.host)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(query.to_string())
            .send()
            .await?;

        match res.json::<GQLResponse<CreateRoomData>>().await {
            Ok(result) => {
                println!("Response from server: {result:?}");
                let desc_data = decode_b64(result.data.create_room.as_str())?;
                let answer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
                return Ok(answer);
            }
            Err(msg) => {
                bail!("Received unexpected response from server: {msg}")
            }
        }
    }

    pub async fn join_room(
        &self,
        name: &str,
        room_type: &PublicRoomType,
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
            .post(&self.host)
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
                println!("ERR joinRoom response: {err}\n{text}");
                Err(err.into())
            }
        }
    }
}
