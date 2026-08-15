use serde::{Deserialize, Serialize};

/// The string names are deliberately explicit: Tauri deserializes the value
/// passed by the frontend, so these remain stable even if Rust variant names change.
#[derive(Clone, Copy, Debug, Deserialize)]
pub enum Provider {
    #[serde(rename = "moviebox", alias = "Moviebox", alias = "MovieBox")]
    Moviebox,
    #[serde(rename = "fourkhdhub", alias = "Fourkhdhub", alias = "4KHDHub")]
    Fourkhdhub,
    #[serde(rename = "circleftp", alias = "Circleftp", alias = "CircleFTP")]
    Circleftp,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub year: String,
    pub poster: Option<String>,
    pub seasons: u32,
    pub rating: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub season: u32,
    pub episode: u32,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dub {
    pub id: String,
    pub language: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub year: String,
    pub poster: Option<String>,
    pub synopsis: String,
    pub rating: String,
    pub genres: Vec<String>,
    pub episodes: Vec<Episode>,
    pub dubs: Vec<Dub>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub id: String,
    pub label: String,
    pub resolution: String,
    pub url: String,
    pub headers: Vec<StreamHeader>,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind")]
pub enum DownloadStatusInfo {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "downloading", rename_all = "camelCase")]
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        speed_bps: u64,
    },
    #[serde(rename = "paused", rename_all = "camelCase")]
    Paused { downloaded: u64, total: Option<u64> },
    #[serde(rename = "completed", rename_all = "camelCase")]
    Completed { size: u64 },
    #[serde(rename = "failed", rename_all = "camelCase")]
    Failed { error: String },
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    pub id: String,
    pub title: String,
    pub episode_label: Option<String>,
    pub resolution: String,
    pub status: DownloadStatusInfo,
    pub file_path: String,
}

#[cfg(test)]
mod tests {
    use super::Provider;

    #[test]
    fn provider_values_accept_canonical_and_saved_legacy_settings() {
        assert!(matches!(
            serde_json::from_str::<Provider>("\"moviebox\"").unwrap(),
            Provider::Moviebox
        ));
        assert!(matches!(
            serde_json::from_str::<Provider>("\"Moviebox\"").unwrap(),
            Provider::Moviebox
        ));
        assert!(matches!(
            serde_json::from_str::<Provider>("\"circleftp\"").unwrap(),
            Provider::Circleftp
        ));
        assert!(matches!(
            serde_json::from_str::<Provider>("\"fourkhdhub\"").unwrap(),
            Provider::Fourkhdhub
        ));
    }

    #[test]
    fn test_download_info_serialization() {
        use super::{DownloadInfo, DownloadStatusInfo};
        let info = DownloadInfo {
            id: "123".into(),
            title: "Test".into(),
            episode_label: None,
            resolution: "1080p".into(),
            status: DownloadStatusInfo::Downloading {
                downloaded: 100,
                total: Some(1000),
                speed_bps: 500,
            },
            file_path: "C:\\test.mp4".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        println!("Serialized: {}", json);
    }
}
