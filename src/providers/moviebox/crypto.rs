use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use rand::Rng;
use reqwest::Method;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::error::{AppError, Result};

type HmacMd5 = Hmac<Md5>;

const SECRET_KEY: &str = "76iRl07s0xSN9jqmEWAt79EBJZulIQIsV64FZr2O";
const VERSION_CODE: u32 = 50_020_044;
const ANDROID_VERSION: &str = "13";
const MODEL: &str = "M2101K6G";

/// A single Android identity is kept for the life of the app. MovieBox's
/// mobile API expects a realistic and internally consistent client profile.
pub struct DeviceProfile {
    device_id: String,
    gaid: String,
}

impl DeviceProfile {
    pub fn new() -> Self {
        Self {
            device_id: format!("{:032x}", rand::thread_rng().r#gen::<u128>()),
            gaid: Uuid::new_v4().to_string(),
        }
    }

    pub fn client_info(&self) -> String {
        json!({
            "package_name": "com.community.oneroom",
            "version_name": "3.0.03.0529.03",
            "version_code": VERSION_CODE,
            "os": "android",
            "os_version": ANDROID_VERSION,
            "install_ch": "ps",
            "device_id": self.device_id,
            "install_store": "ps",
            "gaid": self.gaid,
            "brand": "Redmi",
            "model": MODEL,
            "system_language": "en",
            "net": "NETWORK_WIFI",
            "region": "US",
            "timezone": "Asia/Dhaka",
            "sp_code": "40401",
            "X-Play-Mode": "2"
        })
        .to_string()
    }

    pub fn user_agent(&self) -> String {
        format!(
            "com.community.oneroom/{VERSION_CODE} (Linux; U; Android {ANDROID_VERSION}; en_US; {MODEL}; Build/TP1A.220624.014; Cronet/135.0.7012.3)"
        )
    }

    pub fn forwarded_for(&self) -> String {
        let mut rng = rand::thread_rng();
        format!(
            "103.241.{}.{}",
            rng.gen_range(1..=254),
            rng.gen_range(1..=254)
        )
    }
}

pub fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn client_token(timestamp: u128) -> String {
    let reversed: String = timestamp.to_string().chars().rev().collect();
    format!("{timestamp},{:x}", Md5::digest(reversed))
}

/// Build the MovieBox HMAC-MD5 header from the canonical request format
pub fn request_signature(
    method: &Method,
    canonical_url: &str,
    body: Option<&str>,
    timestamp: u128,
) -> Result<String> {
    let body = body.unwrap_or("").as_bytes();
    let body_length = if body.is_empty() {
        String::new()
    } else {
        body.len().to_string()
    };
    let body_md5 = if body.is_empty() {
        String::new()
    } else {
        format!("{:x}", Md5::digest(&body[..body.len().min(102_400)]))
    };
    let canonical = format!(
        "{method}\napplication/json\napplication/json\n{body_length}\n{timestamp}\n{body_md5}\n{canonical_url}"
    );
    let secret = STANDARD
        .decode(SECRET_KEY)
        .map_err(|error| AppError::Message(error.to_string()))?;
    let mut mac =
        HmacMd5::new_from_slice(&secret).map_err(|error| AppError::Message(error.to_string()))?;
    mac.update(canonical.as_bytes());

    Ok(format!(
        "{timestamp}|2|{}",
        STANDARD.encode(mac.finalize().into_bytes())
    ))
}
