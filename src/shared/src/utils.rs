
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

pub fn encode_offer(offer: RTCSessionDescription) -> anyhow::Result<String> {
    let json = serde_json::to_string(&offer)?;
    Ok(BASE64_STANDARD.encode(json))
}

pub fn decode_b64(s: &str) -> anyhow::Result<String> {
    let b = BASE64_STANDARD.decode(s)?;

    let s = String::from_utf8(b)?;
    Ok(s)
}
