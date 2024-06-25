use anyhow::Result;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;

/// encode encodes the input in base64
pub fn encode(b: &str) -> String {
    BASE64_STANDARD.encode(b)
}

/// decode decodes the input from base64
pub fn decode(s: &str) -> Result<String> {
    let b = BASE64_STANDARD.decode(s)?;

    let s = String::from_utf8(b)?;
    Ok(s)
}
