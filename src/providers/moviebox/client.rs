use std::collections::{BTreeMap, HashSet};

use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use url::Url;

use crate::{
    error::{AppError, Result},
    models::{Details, Episode, MediaItem, Stream},
};

use super::crypto::{DeviceProfile, client_token, request_signature, timestamp_ms};

const HOST_POOL: [&str; 7] = [
    "https://api6.aoneroom.com",
    "https://api5.aoneroom.com",
    "https://api4.aoneroom.com",
    "https://api4sg.aoneroom.com",
    "https://api3.aoneroom.com",
    "https://api6sg.aoneroom.com",
    "https://api.inmoviebox.com",
];
const RETRYABLE_STATUSES: [StatusCode; 8] = [
    StatusCode::FORBIDDEN,
    StatusCode::NOT_ACCEPTABLE,
    StatusCode::PROXY_AUTHENTICATION_REQUIRED,
    StatusCode::TOO_MANY_REQUESTS,
    StatusCode::INTERNAL_SERVER_ERROR,
    StatusCode::BAD_GATEWAY,
    StatusCode::SERVICE_UNAVAILABLE,
    StatusCode::GATEWAY_TIMEOUT,
];

pub struct MovieBoxClient {
    http: Client,
    token: Option<String>,
    profile: DeviceProfile,
}

impl MovieBoxClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(12))
            .connect_timeout(std::time::Duration::from_secs(3))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .build()
            .expect("MovieBox HTTP client");

        Self {
            http,
            token: None,
            profile: DeviceProfile::new(),
        }
    }

    pub async fn search(&mut self, query: &str) -> Result<Vec<MediaItem>> {
        let response = self
            .request(
                Method::POST,
                "/wefeed-mobile-bff/subject-api/search/v2",
                Some(json!({
                    "keyword": query,
                    "page": 1,
                    "perPage": 20,
                    "subjectType": "All",
                    "tabId": "All"
                })),
            )
            .await?;

        let mut results = Vec::new();
        for group in response["results"].as_array().into_iter().flatten() {
            for subject in group["subjects"].as_array().into_iter().flatten() {
                if let Some(item) = media_item(subject) {
                    if let Some(existing) = results.iter_mut().find(|entry: &&mut MediaItem| {
                        entry.title == item.title && entry.kind == item.kind
                    }) {
                        existing.seasons = existing.seasons.max(item.seasons);
                    } else {
                        results.push(item);
                    }
                }
            }
        }

        let query = query.to_ascii_lowercase();
        results.sort_by(|left, right| {
            search_rank(left, &query)
                .cmp(&search_rank(right, &query))
                .then_with(|| right.year.cmp(&left.year))
        });
        Ok(results)
    }

    pub async fn details(&mut self, id: &str) -> Result<Details> {
        let data = self
            .request(
                Method::GET,
                &format!("/wefeed-mobile-bff/subject-api/get?subjectId={id}"),
                None,
            )
            .await?;
        let item = media_item(&data)
            .ok_or_else(|| AppError::Message("MovieBox returned incomplete details.".into()))?;

        let episodes = if item.kind == "series" {
            self.fetch_episodes(id, &data).await
        } else {
            Vec::new()
        };

        Ok(Details {
            id: item.id,
            title: item.title,
            kind: item.kind,
            year: item.year,
            poster: item.poster,
            synopsis: data["synopsis"]
                .as_str()
                .or(data["description"].as_str())
                .unwrap_or("No description available.")
                .to_owned(),
            rating: value_text(&data["imdbRatingValue"]).unwrap_or_else(|| "—".into()),
            genres: string_values(&data["genre"]),
            episodes,
        })
    }

    pub async fn streams(
        &mut self,
        id: &str,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Result<Vec<Stream>> {
        match (season, episode) {
            (Some(season), Some(episode)) => self.series_streams(id, season, episode).await,
            _ => self.movie_streams(id).await,
        }
    }

    async fn fetch_episodes(&mut self, id: &str, details: &Value) -> Vec<Episode> {
        let seasons = self
            .request(
                Method::GET,
                &format!("/wefeed-mobile-bff/subject-api/season-info?subjectId={id}"),
                None,
            )
            .await
            .ok()
            .and_then(|response| response["seasons"].as_array().cloned());

        if let Some(seasons) = seasons {
            return seasons
                .iter()
                .flat_map(episodes_from_season)
                .collect::<Vec<_>>();
        }

        // Older MovieBox entries expose episode count only through
        // resourceDetectors; the analysis specifies a synthetic first season.
        let total = details["resourceDetectors"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["totalEpisode"].as_u64())
            .unwrap_or(0) as u32;
        (1..=total)
            .map(|episode| Episode {
                season: 1,
                episode,
                label: format!("S01 · E{episode:02}"),
            })
            .collect()
    }

    async fn movie_streams(&mut self, id: &str) -> Result<Vec<Stream>> {
        let mut streams = Vec::new();
        for page in 1..=10 {
            let response = self.resource_request(id, None, None, page, None).await?;
            streams.extend(streams_from_response(&response));
            if !has_more(&response) {
                break;
            }
        }
        Ok(deduplicate_and_sort(streams))
    }

    async fn series_streams(&mut self, id: &str, season: u32, episode: u32) -> Result<Vec<Stream>> {
        // collectionResolutions is fetched before per-episode requests, as in
        // the source analysis. A no-resolution fallback serves older entries.
        let initial = self.resource_request(id, None, None, 1, None).await?;
        let mut resolutions = initial["collectionResolutions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["resolution"].as_u64().map(|value| value as u32))
            .collect::<Vec<_>>();
        resolutions.sort_unstable_by(|left, right| right.cmp(left));
        resolutions.dedup();
        if resolutions.is_empty() {
            resolutions.push(0);
        }

        let mut streams = Vec::new();
        for page in 1..=60 {
            let mut page_has_more = false;
            for resolution in &resolutions {
                let response = self
                    .resource_request(
                        id,
                        Some(season),
                        Some(episode),
                        page,
                        (*resolution != 0).then_some(*resolution),
                    )
                    .await?;
                page_has_more |= has_more(&response);
                streams.extend(
                    streams_from_response(&response)
                        .into_iter()
                        .filter(|stream| {
                            // The server normally scopes this endpoint itself; stream
                            // IDs are preserved even when metadata omits se/ep.
                            !stream.url.is_empty()
                        }),
                );
            }
            if !streams.is_empty() || !page_has_more {
                break;
            }
        }
        Ok(deduplicate_and_sort(streams))
    }

    async fn resource_request(
        &mut self,
        id: &str,
        season: Option<u32>,
        episode: Option<u32>,
        page: u32,
        resolution: Option<u32>,
    ) -> Result<Value> {
        let mut path = format!(
            "/wefeed-mobile-bff/subject-api/resource?subjectId={id}&page={page}&perPage=20"
        );
        if let (Some(season), Some(episode)) = (season, episode) {
            path.push_str(&format!("&se={season}&ep={episode}"));
        }
        if let Some(resolution) = resolution {
            path.push_str(&format!("&resolution={resolution}"));
        }
        self.request(Method::GET, &path, None).await
    }

    async fn request(&mut self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let first_try = self
            .request_from_hosts(method.clone(), path, body.clone())
            .await;
        if first_try.is_ok() || self.token.is_some() {
            return first_try;
        }

        // If the initial host round could not establish an x-user bearer token,
        // request the documented tab-operating initialization route and retry.
        self.initialize().await?;
        self.request_from_hosts(method, path, body).await
    }

    async fn initialize(&mut self) -> Result<()> {
        self.request_from_hosts(
            Method::GET,
            "/wefeed-mobile-bff/tab-operating?page=1&tabId=0&version=",
            None,
        )
        .await?;
        if self.token.is_none() {
            return Err(AppError::Message(
                "MovieBox did not provide an authentication token.".into(),
            ));
        }
        Ok(())
    }

    async fn request_from_hosts(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let body_text = body.as_ref().map(Value::to_string);
        let mut last_error = "MovieBox did not return a usable response.".to_owned();

        for host in HOST_POOL {
            let url = format!("{host}{path}");
            let canonical_url = canonical_url(&url)?;
            let timestamp = timestamp_ms();
            let mut request = self
                .http
                .request(method.clone(), &url)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("x-client-token", client_token(timestamp))
                .header(
                    "x-tr-signature",
                    request_signature(&method, &canonical_url, body_text.as_deref(), timestamp)?,
                )
                .header("x-client-info", self.profile.client_info())
                .header("x-client-status", "0")
                .header("x-forwarded-for", self.profile.forwarded_for())
                .header("User-Agent", self.profile.user_agent());
            if let Some(token) = &self.token {
                request = request.bearer_auth(token);
            }
            if let Some(body) = &body {
                request = request.json(body);
            }

            match request.send().await {
                Ok(response) if response.status().is_success() => {
                    self.store_token(response.headers());
                    let body: Value = response.json().await?;
                    return Ok(body.get("data").cloned().unwrap_or(body));
                }
                Ok(response) if RETRYABLE_STATUSES.contains(&response.status()) => {
                    last_error = format!("MovieBox returned {}", response.status());
                }
                Ok(response) => {
                    return Err(AppError::Message(format!(
                        "MovieBox returned {}",
                        response.status()
                    )));
                }
                Err(error) => last_error = error.to_string(),
            }
        }
        Err(AppError::Message(last_error))
    }

    fn store_token(&mut self, headers: &reqwest::header::HeaderMap) {
        if self.token.is_some() {
            return;
        }
        self.token = headers
            .get("x-user")
            .and_then(|header| header.to_str().ok())
            .and_then(|header| serde_json::from_str::<Value>(header).ok())
            .and_then(|user| user["token"].as_str().map(str::to_owned));
    }
}

fn canonical_url(raw_url: &str) -> Result<String> {
    let url = Url::parse(raw_url).map_err(|error| AppError::Message(error.to_string()))?;
    let mut parameters = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        parameters.insert(key.into_owned(), value.into_owned());
    }
    let query = parameters
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    Ok(if query.is_empty() {
        url.path().to_owned()
    } else {
        format!("{}?{query}", url.path())
    })
}

fn media_item(value: &Value) -> Option<MediaItem> {
    Some(MediaItem {
        id: value["subjectId"].as_str()?.to_owned(),
        title: clean_title(value["title"].as_str().unwrap_or("Untitled")),
        kind: if value["subjectType"].as_i64() == Some(2) {
            "series".into()
        } else {
            "movie".into()
        },
        year: value_text(&value["releaseDate"])
            .map(|d| d.get(..4).unwrap_or(&d).to_owned())
            .unwrap_or_else(|| "—".into()),
        poster: value["poster"]
            .as_str()
            .or(value["cover"]["url"].as_str())
            .or(value["pic"].as_str())
            .map(str::to_owned),
        seasons: value["season"].as_u64().unwrap_or(0) as u32,
    })
}

fn clean_title(raw: &str) -> String {
    let without_bracket = raw.split(" [").next().unwrap_or(raw).trim();
    let lower = without_bracket.to_ascii_lowercase();
    for suffix in [" (dub)", " (hindi)"] {
        if lower.ends_with(suffix) {
            return without_bracket[..without_bracket.len() - suffix.len()]
                .trim()
                .to_owned();
        }
    }
    without_bracket.to_owned()
}

fn search_rank(item: &MediaItem, query: &str) -> (u8, u8) {
    let title = item.title.to_ascii_lowercase();
    let match_rank = if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else {
        2
    };
    (match_rank, u8::from(item.kind != "series"))
}

fn episodes_from_season(season: &Value) -> Vec<Episode> {
    let season_number = season["se"].as_u64().unwrap_or(1) as u32;
    let raw_numbers = season["allEp"].as_str().unwrap_or("");
    let numbers: Vec<u32> = if raw_numbers.is_empty() {
        (1..=season["maxEp"].as_u64().unwrap_or(0) as u32).collect()
    } else {
        raw_numbers
            .split(',')
            .filter_map(|value| value.trim().parse::<u32>().ok())
            .collect()
    };
    numbers
        .into_iter()
        .map(|episode| Episode {
            season: season_number,
            episode,
            label: format!("S{season_number:02} · E{episode:02}"),
        })
        .collect()
}

fn streams_from_response(response: &Value) -> Vec<Stream> {
    response["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|resource| {
            let url = resource["resourceLink"].as_str()?.to_owned();
            let size_bytes = resource["size"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| resource["size"].as_u64());
            Some(Stream {
                id: resource["resourceId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                label: resource["title"]
                    .as_str()
                    .or(resource["fileName"].as_str())
                    .unwrap_or("Stream")
                    .to_owned(),
                resolution: resource["resolution"]
                    .as_u64()
                    .map(|value| format!("{value}p"))
                    .unwrap_or_else(|| "Auto".into()),
                url,
                headers: vec![],
                size_bytes,
            })
        })
        .collect()
}

fn has_more(response: &Value) -> bool {
    response["pager"]["hasMore"].as_bool().unwrap_or(false)
}

fn deduplicate_and_sort(streams: Vec<Stream>) -> Vec<Stream> {
    let mut seen_ids = HashSet::new();
    let mut unique_resources = streams
        .into_iter()
        .filter(|stream| seen_ids.insert(stream.id.clone()))
        .collect::<Vec<_>>();

    // MovieBox frequently returns several CDN/source copies of the same
    // encoding. The picker is resolution-oriented, so retain only the first
    // valid resource for each resolution rather than showing a long list of
    // indistinguishable 1080p or 720p entries.
    unique_resources.sort_by(|left, right| {
        resolution_value(&right.resolution).cmp(&resolution_value(&left.resolution))
    });
    let mut one_per_resolution = BTreeMap::new();
    for stream in unique_resources {
        one_per_resolution
            .entry(stream.resolution.clone())
            .or_insert(stream);
    }

    let mut streams = one_per_resolution.into_values().collect::<Vec<_>>();
    streams.sort_by(|left, right| {
        resolution_value(&right.resolution).cmp(&resolution_value(&left.resolution))
    });
    streams
}

fn resolution_value(resolution: &str) -> u32 {
    resolution.trim_end_matches('p').parse().unwrap_or(0)
}

fn value_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_f64().map(|number| number.to_string()))
}

fn string_values(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(value_text).collect())
        .or_else(|| value_text(value).map(|text| vec![text]))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::deduplicate_and_sort;
    use crate::models::Stream;

    #[test]
    fn stream_picker_keeps_one_stream_for_each_resolution() {
        let streams = vec![
            stream("a", "1080p"),
            stream("b", "1080p"),
            stream("c", "720p"),
            stream("d", "720p"),
            stream("e", "480p"),
        ];

        let streams = deduplicate_and_sort(streams);
        assert_eq!(
            streams
                .iter()
                .map(|stream| &stream.resolution)
                .collect::<Vec<_>>(),
            vec!["1080p", "720p", "480p"]
        );
    }

    fn stream(id: &str, resolution: &str) -> Stream {
        Stream {
            id: id.into(),
            label: "test".into(),
            resolution: resolution.into(),
            url: format!("https://example.test/{id}"),
            headers: vec![],
            size_bytes: None,
        }
    }
}
