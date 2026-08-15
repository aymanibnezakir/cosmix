mod client;
mod parser;
mod resolver;

use crate::{
    error::Result,
    models::{Details, MediaItem, Stream},
};

pub async fn search(query: &str) -> Result<Vec<MediaItem>> {
    client::FourKHdHubClient::new()?.search(query).await
}

pub async fn details(id: &str) -> Result<Details> {
    client::FourKHdHubClient::new()?.details(id).await
}

pub async fn streams(id: &str, season: Option<u32>, episode: Option<u32>) -> Result<Vec<Stream>> {
    client::FourKHdHubClient::new()?
        .streams(id, season, episode)
        .await
}
