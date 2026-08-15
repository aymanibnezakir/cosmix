use std::env;

use reqwest::{Client, redirect::Policy};
use url::Url;

use crate::{
    error::{AppError, Result},
    models::{Details, MediaItem, Stream, StreamHeader},
};

use super::{parser, resolver};

pub(super) const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub(super) struct FourKHdHubClient {
    http: Client,
    base_url: String,
}

impl FourKHdHubClient {
    pub(super) fn new() -> Result<Self> {
        let base_url =
            env::var("MOVIEBOX_FOURKHDHUB_URL").unwrap_or_else(|_| "https://4khdhub.one/".into());
        let base_url = ensure_trailing_slash(&base_url)?;
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(4))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(6)
            .user_agent(BROWSER_USER_AGENT)
            .redirect(Policy::limited(5))
            .build()?;
        Ok(Self { http, base_url })
    }

    pub(super) async fn search(&self, query: &str) -> Result<Vec<MediaItem>> {
        let mut url =
            Url::parse(&self.base_url).map_err(|error| AppError::Message(error.to_string()))?;
        url.query_pairs_mut().append_pair("s", query);
        let html = self.fetch_html(url.as_str()).await?;
        Ok(parser::search_results(&html, &self.base_url))
    }

    pub(super) async fn details(&self, id: &str) -> Result<Details> {
        let html = self.fetch_html(&self.page_url(id)?).await?;
        Ok(parser::details(&html, id, &self.base_url))
    }

    pub(super) async fn streams(
        &self,
        id: &str,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<Vec<Stream>> {
        use futures_util::stream::{self, StreamExt};

        let html = self.fetch_html(&self.page_url(id)?).await?;
        let releases = parser::releases(&html, &self.base_url, season, episode);
        if releases.is_empty() {
            return Err(AppError::Message(
                "4KHDHub did not expose a downloadable stream for this selection.".into(),
            ));
        }

        let headers = self.playback_headers();
        let http = self.http.clone();

        let mut streams: Vec<Stream> = stream::iter(releases)
            .map(|candidate| {
                let http = http.clone();
                let headers = headers.clone();
                async move {
                    if let Some((url, size_bytes)) =
                        resolver::resolve_and_preflight(&http, &candidate.resolver_url, &headers)
                            .await
                    {
                        Some(Stream {
                            id: candidate.resolver_url,
                            label: candidate.label,
                            resolution: candidate.resolution,
                            url,
                            headers,
                            size_bytes,
                        })
                    } else {
                        None
                    }
                }
            })
            .buffer_unordered(8)
            .filter_map(|stream_opt| async move { stream_opt })
            .collect()
            .await;

        if streams.is_empty() {
            return Err(AppError::Message(
                "4KHDHub returned links, but none resolved to a playable video stream.".into(),
            ));
        }

        let mut seen = std::collections::HashSet::new();
        streams.retain(|s| seen.insert(s.url.clone()));

        streams.sort_by(|left, right| {
            resolution_value(&right.resolution).cmp(&resolution_value(&left.resolution))
        });
        Ok(streams)
    }

    async fn fetch_html(&self, url: &str) -> Result<String> {
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.text().await?)
    }

    fn page_url(&self, id: &str) -> Result<String> {
        let base =
            Url::parse(&self.base_url).map_err(|error| AppError::Message(error.to_string()))?;
        let url = Url::parse(id)
            .ok()
            .filter(|url| url.host_str().is_some())
            .unwrap_or(
                base.join(id)
                    .map_err(|error| AppError::Message(error.to_string()))?,
            );
        if url.host_str() != base.host_str() {
            return Err(AppError::Message(
                "4KHDHub result does not belong to the configured provider site.".into(),
            ));
        }
        Ok(url.into())
    }

    fn playback_headers(&self) -> Vec<StreamHeader> {
        vec![
            StreamHeader {
                name: "Referer".into(),
                value: self.base_url.clone(),
            },
            StreamHeader {
                name: "User-Agent".into(),
                value: BROWSER_USER_AGENT.into(),
            },
        ]
    }
}

fn ensure_trailing_slash(raw: &str) -> Result<String> {
    let mut url = Url::parse(raw).map_err(|error| {
        AppError::Message(format!("MOVIEBOX_FOURKHDHUB_URL is invalid: {error}"))
    })?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return Err(AppError::Message(
            "MOVIEBOX_FOURKHDHUB_URL must be an HTTP(S) site URL.".into(),
        ));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url.into())
}

fn resolution_value(resolution: &str) -> u32 {
    resolution.trim_end_matches('p').parse().unwrap_or(0)
}
