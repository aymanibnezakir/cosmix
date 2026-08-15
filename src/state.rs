use tokio::sync::Mutex;

use crate::{download::DownloadManager, providers::moviebox::MovieBoxClient};

pub struct AppState {
    pub moviebox: Mutex<MovieBoxClient>,
    pub downloads: DownloadManager,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            moviebox: Mutex::new(MovieBoxClient::new()),
            downloads: DownloadManager::new(),
        }
    }
}
