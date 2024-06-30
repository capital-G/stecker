use anyhow::bail;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use shared::utils::decode_b64;
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

impl APIClient {
    pub async fn create_room(
        &self,
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
                println!("ERR joinRoom response: {err}");
                Err(err.into())
            }
        }
    }
}
