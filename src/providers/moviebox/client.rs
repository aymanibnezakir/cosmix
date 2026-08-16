use std::collections::{BTreeMap, HashSet};

use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use url::Url;

use crate::{
    error::{AppError, Result},
    models::{Details, Dub, Episode, MediaItem, Stream},
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

        let mut results: Vec<MediaItem> = Vec::new();
        let mut is_dub_list: Vec<bool> = Vec::new();

        for group in response["results"].as_array().into_iter().flatten() {
            for subject in group["subjects"].as_array().into_iter().flatten() {
                let subject_type = subject["subjectType"].as_i64().unwrap_or(1);
                if subject_type != 1 && subject_type != 2 {
                    continue;
                }

                if let Some(item) = media_item(subject) {
                    let raw_title = subject["title"].as_str().unwrap_or("");
                    let corner = subject["corner"].as_str().unwrap_or("");
                    let is_dub = is_dub_entry(raw_title, corner);

                    if let Some(idx) = results.iter().position(|entry| {
                        if entry.id == item.id {
                            return true;
                        }
                        if entry.title.eq_ignore_ascii_case(&item.title) && entry.kind == item.kind {
                            if item.kind == "series" {
                                return true;
                            }
                            return entry.year == item.year || entry.year == "—" || item.year == "—";
                        }
                        false
                    }) {
                        results[idx].seasons = results[idx].seasons.max(item.seasons);

                        // For series, prefer the earliest release year (e.g. premiere year 2008 over 2014)
                        if item.kind == "series" && item.year != "—" {
                            if results[idx].year == "—" || item.year < results[idx].year {
                                results[idx].year = item.year.clone();
                            }
                        }

                        // If the existing entry was a dub, but this new item is original, replace metadata
                        if is_dub_list[idx] && !is_dub {
                            let seasons = results[idx].seasons;
                            let year = if item.kind == "series" && results[idx].year != "—" && results[idx].year < item.year {
                                results[idx].year.clone()
                            } else {
                                item.year.clone()
                            };
                            results[idx] = item;
                            results[idx].seasons = seasons;
                            results[idx].year = year;
                            is_dub_list[idx] = false;
                        }
                    } else {
                        results.push(item);
                        is_dub_list.push(is_dub);
                    }
                }
            }
        }

        let query_lower = query.trim().to_lowercase();
        results.retain(|item| item.title.to_lowercase().contains(&query_lower));

        results.sort_by(|left, right| {
            search_rank(left, &query_lower)
                .cmp(&search_rank(right, &query_lower))
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

        let dubs = parse_dubs(&data);

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
            dubs,
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
        let seasons = self
            .request(
                Method::GET,
                &format!("/wefeed-mobile-bff/subject-api/season-info?subjectId={id}"),
                None,
            )
            .await
            .ok()
            .and_then(|response| response["seasons"].as_array().cloned())
            .unwrap_or_default();

        let mut absolute_episode = 0;
        for s in &seasons {
            let s_num = s["se"].as_u64().unwrap_or(0) as u32;
            let max_ep = s["maxEp"].as_u64().unwrap_or(0) as u32;
            if s_num < season {
                absolute_episode += max_ep;
            }
        }
        absolute_episode += episode.saturating_sub(1);
        let estimated_page = (absolute_episode / 20) + 1;

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
        let start_page = estimated_page.saturating_sub(1).max(1);
        let end_page = start_page + 10;

        for page in start_page..=end_page {
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

                if let Some(list) = response["list"].as_array() {
                    for resource in list {
                        let item_se = resource["se"].as_u64().map(|v| v as u32);
                        let item_ep = resource["ep"].as_u64().map(|v| v as u32);

                        if let (Some(se), Some(ep)) = (item_se, item_ep) {
                            if se != season || ep != episode {
                                continue;
                            }
                        }

                        if let Some(stream) = stream_from_resource(resource) {
                            streams.push(stream);
                        }
                    }
                }
            }
            if !streams.is_empty() || !page_has_more {
                break;
            }
        }

        // Fallback: scan remaining pages from 1 to start_page if not found in the estimated window
        if streams.is_empty() && start_page > 1 {
            for page in 1..start_page {
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

                    if let Some(list) = response["list"].as_array() {
                        for resource in list {
                            let item_se = resource["se"].as_u64().map(|v| v as u32);
                            let item_ep = resource["ep"].as_u64().map(|v| v as u32);

                            if let (Some(se), Some(ep)) = (item_se, item_ep) {
                                if se != season || ep != episode {
                                    continue;
                                }
                            }

                            if let Some(stream) = stream_from_resource(resource) {
                                streams.push(stream);
                            }
                        }
                    }
                }
                if !streams.is_empty() || !page_has_more {
                    break;
                }
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
        rating: value_text(&value["imdbRatingValue"])
            .or_else(|| value_text(&value["imdbRating"]))
            .or_else(|| value_text(&value["rating"]))
            .filter(|r| r != "0" && r != "0.0" && r != "—" && !r.is_empty()),
    })
}

fn clean_title(raw: &str) -> String {
    let mut title = raw.trim().to_string();

    // 1. Strip bracketed expressions, e.g. [Hindi], [Dual Audio], [Tamil], etc.
    while let Some(start) = title.find('[') {
        if let Some(end) = title[start..].find(']') {
            let before = title[..start].to_string();
            let after = title[start + end + 1..].to_string();
            title = format!("{before} {after}").trim().to_string();
        } else {
            title = title[..start].trim().to_string();
        }
    }

    // 2. Strip parenthetical expressions that indicate dubs or seasons, e.g. (Hindi Dub), (Dubbed), (Hindi)
    while let Some(start) = title.find('(') {
        if let Some(end) = title[start..].find(')') {
            let inside = title[start + 1..start + end].trim();
            if is_dub_or_season_tag(inside) {
                let before = title[..start].to_string();
                let after = title[start + end + 1..].to_string();
                title = format!("{before} {after}").trim().to_string();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // 3. Strip trailing season indicators like "S01-02", "S01-S05", "S01", "S1", "Season 1"
    let mut words: Vec<String> = title.split_whitespace().map(|s| s.to_string()).collect();
    while let Some(last) = words.last() {
        if is_season_tag(last) {
            words.pop();
        } else {
            break;
        }
    }
    let cleaned = words.join(" ");
    let cleaned = cleaned
        .trim_end_matches(|c| c == '-' || c == ':' || c == '|' || c == ',' || c == '.')
        .trim();

    if cleaned.is_empty() {
        raw.trim().to_owned()
    } else {
        cleaned.to_owned()
    }
}

fn is_dub_or_season_tag(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    lower.contains("dub")
        || lower.contains("audio")
        || lower.contains("hindi")
        || lower.contains("tamil")
        || lower.contains("telugu")
        || lower.contains("spanish")
        || lower.contains("english")
        || lower.contains("french")
        || lower.contains("german")
        || lower.contains("portuguese")
        || lower.contains("sub")
        || lower.contains("season")
        || is_season_tag(&lower)
}

fn is_season_tag(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    let lower = lower.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
    if lower.starts_with('s') && lower.len() >= 2 {
        let rest = &lower[1..];
        if rest.chars().all(|c| c.is_ascii_digit() || c == '-' || c == 's') {
            return true;
        }
    }
    if lower.starts_with("season") {
        return true;
    }
    false
}

fn is_dub_entry(raw_title: &str, corner: &str) -> bool {
    if !corner.is_empty() {
        return true;
    }
    if raw_title.contains('[') || raw_title.contains(']') {
        return true;
    }
    let lower = raw_title.to_ascii_lowercase();
    lower.contains("dub")
        || lower.contains("hindi")
        || lower.contains("tamil")
        || lower.contains("telugu")
        || lower.contains("dual audio")
        || lower.contains("multi audio")
}

fn resolve_language_name(code: &str) -> Option<&'static str> {
    match code.to_ascii_lowercase().as_str() {
        "en" | "eng" => Some("English"),
        "hi" | "hin" => Some("Hindi"),
        "es" | "spa" => Some("Spanish"),
        "esla" => Some("Spanish (Latin America)"),
        "pt" | "por" => Some("Portuguese"),
        "ptbr" => Some("Portuguese (Brazil)"),
        "ta" | "tam" => Some("Tamil"),
        "te" | "tel" => Some("Telugu"),
        "ml" | "mal" => Some("Malayalam"),
        "kn" | "kan" => Some("Kannada"),
        "bn" | "ben" => Some("Bengali"),
        "fr" | "fra" | "fre" => Some("French"),
        "de" | "deu" | "ger" => Some("German"),
        "it" | "ita" => Some("Italian"),
        "ja" | "jpn" => Some("Japanese"),
        "ko" | "kor" => Some("Korean"),
        "ru" | "rus" => Some("Russian"),
        "zh" | "zho" | "chi" => Some("Chinese"),
        "id" | "ind" => Some("Indonesian"),
        "th" | "tha" => Some("Thai"),
        "vi" | "vie" => Some("Vietnamese"),
        "tr" | "tur" => Some("Turkish"),
        "ar" | "ara" => Some("Arabic"),
        _ => None,
    }
}

fn format_language_name(lan_name: &str, lan_code: &str, original: bool) -> String {
    let lower_name = lan_name.to_ascii_lowercase();

    if original
        || lower_name == "original"
        || lower_name == "original audio"
        || lower_name.starts_with("original")
    {
        if let Some(lang) = resolve_language_name(lan_code) {
            return format!("{lang} (Original)");
        }
        if let Some(start) = lan_name.find('(') {
            if let Some(end) = lan_name[start..].find(')') {
                let inside = lan_name[start + 1..start + end].trim();
                if !inside.is_empty() {
                    return format!("{inside} (Original)");
                }
            }
        }
        return "Original Audio".to_string();
    }

    if let Some(lang) = resolve_language_name(lan_code) {
        return lang.to_string();
    }

    if !lan_name.is_empty() {
        let name = lan_name.trim();
        let clean = if let Some(stripped) = name.strip_suffix(" dub") {
            stripped
        } else if let Some(stripped) = name.strip_suffix(" Dub") {
            stripped
        } else {
            name
        };
        let words: Vec<String> = clean
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect();
        words.join(" ")
    } else if !lan_code.is_empty() {
        lan_code.to_uppercase()
    } else {
        "Dubbed".to_string()
    }
}

fn parse_dubs(data: &Value) -> Vec<Dub> {
    let mut dubs = Vec::new();
    let mut seen_ids = HashSet::new();

    if let Some(items) = data["dubs"].as_array() {
        for item in items {
            if let Some(subject_id) = item["subjectId"].as_str() {
                if seen_ids.insert(subject_id.to_string()) {
                    let lan_name = item["lanName"].as_str().unwrap_or("");
                    let lan_code = item["lanCode"].as_str().unwrap_or("");
                    let original = item["original"].as_bool().unwrap_or(false);
                    let language = format_language_name(lan_name, lan_code, original);
                    dubs.push(Dub {
                        id: subject_id.to_string(),
                        language,
                    });
                }
            }
        }
    }

    // Sort dubs so Original audio is at the top, then alphabetically
    dubs.sort_by(|a, b| {
        let a_orig = a.language.contains("(Original)") || a.language.starts_with("Original");
        let b_orig = b.language.contains("(Original)") || b.language.starts_with("Original");
        b_orig.cmp(&a_orig).then_with(|| a.language.cmp(&b.language))
    });

    dubs
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

fn stream_from_resource(resource: &Value) -> Option<Stream> {
    let url = resource["resourceLink"].as_str()?.to_owned();
    if url.is_empty() {
        return None;
    }
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
}

fn streams_from_response(response: &Value) -> Vec<Stream> {
    response["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(stream_from_resource)
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
    use super::{clean_title, deduplicate_and_sort, format_language_name};
    use crate::models::{MediaItem, Stream};

    #[test]
    fn test_search_results_contain_query_keyword_case_insensitive() {
        let items = vec![
            MediaItem {
                id: "1".into(),
                title: "The Dark Knight".into(),
                kind: "movie".into(),
                year: "2008".into(),
                poster: None,
                seasons: 0,
                rating: None,
            },
            MediaItem {
                id: "2".into(),
                title: "Batman Begins".into(),
                kind: "movie".into(),
                year: "2005".into(),
                poster: None,
                seasons: 0,
                rating: None,
            },
            MediaItem {
                id: "3".into(),
                title: "dark".into(),
                kind: "series".into(),
                year: "2017".into(),
                poster: None,
                seasons: 3,
                rating: None,
            },
        ];

        let query = "DARK";
        let query_lower = query.trim().to_lowercase();
        let filtered: Vec<_> = items
            .into_iter()
            .filter(|item| item.title.to_lowercase().contains(&query_lower))
            .collect();

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].title, "The Dark Knight");
        assert_eq!(filtered[1].title, "dark");
    }

    #[test]
    fn test_clean_title_removes_dubs_and_brackets() {
        assert_eq!(clean_title("The Nun [Hindi]"), "The Nun");
        assert_eq!(clean_title("The Nun (Hindi Dub)"), "The Nun");
        assert_eq!(clean_title("Bleach (Hindi Dub) S01-02"), "Bleach");
        assert_eq!(clean_title("Avatar [Tamil] [Telugu]"), "Avatar");
        assert_eq!(clean_title("Money Heist S01-05"), "Money Heist");
        assert_eq!(clean_title("Breaking Bad"), "Breaking Bad");
    }

    #[test]
    fn test_format_language_name() {
        assert_eq!(format_language_name("Original Audio", "en", true), "English (Original)");
        assert_eq!(format_language_name("Original Audio", "ja", true), "Japanese (Original)");
        assert_eq!(format_language_name("Original Audio", "", true), "Original Audio");
        assert_eq!(format_language_name("Hindi dub", "hi", false), "Hindi");
        assert_eq!(format_language_name("esla dub", "esla", false), "Spanish (Latin America)");
        assert_eq!(format_language_name("ptbr dub", "ptbr", false), "Portuguese (Brazil)");
    }

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

    #[tokio::test]
    #[ignore]
    async fn test_the_nun_search_and_dubs() {
        let mut client = super::MovieBoxClient::new();
        let items = client.search("The Nun").await.unwrap();
        println!("Search results count for 'The Nun': {}", items.len());
        for it in &items {
            println!("  Item: id={} title='{}' year='{}' kind='{}'", it.id, it.title, it.year, it.kind);
        }

        // Verify only 1 result for "The Nun" (2018)
        let nun_2018 = items.iter().filter(|it| it.title == "The Nun" && it.year == "2018").collect::<Vec<_>>();
        assert_eq!(nun_2018.len(), 1, "Expected exactly 1 result for The Nun (2018)");

        let details = client.details(&nun_2018[0].id).await.unwrap();
        println!("Details for '{}' ({}): dubs count={}", details.title, details.year, details.dubs.len());
        for dub in &details.dubs {
            println!("  Dub: id={} language='{}'", dub.id, dub.language);
        }
        assert!(details.dubs.len() >= 2, "Expected at least 2 dubs (Original, Hindi, etc.)");
        assert!(details.dubs.iter().any(|d| d.language.contains("Original")));
        assert!(details.dubs.iter().any(|d| d.language.contains("Hindi")));
    }

    #[tokio::test]
    #[ignore]
    async fn test_the_mentalist_search() {
        let mut client = super::MovieBoxClient::new();
        let raw_res = client.request(
            reqwest::Method::POST,
            "/wefeed-mobile-bff/subject-api/search/v2",
            Some(serde_json::json!({
                "keyword": "The Mentalist",
                "page": 1,
                "perPage": 20,
                "subjectType": "All",
                "tabId": "All"
            })),
        ).await.unwrap();

        println!("Raw results for 'The Mentalist':");
        for group in raw_res["results"].as_array().into_iter().flatten() {
            for s in group["subjects"].as_array().into_iter().flatten() {
                println!(
                    "  Raw Subject: id={} title='{}' relDate='{}' type={} corner='{}' season={}",
                    s["subjectId"].as_str().unwrap_or(""),
                    s["title"].as_str().unwrap_or(""),
                    s["releaseDate"].as_str().unwrap_or(""),
                    s["subjectType"].as_i64().unwrap_or(0),
                    s["corner"].as_str().unwrap_or(""),
                    s["season"].as_u64().unwrap_or(0)
                );
            }
        }

        let items = client.search("The Mentalist").await.unwrap();
        println!("\nDeduped search items for 'The Mentalist':");
        for it in &items {
            println!("  Item: id={} title='{}' year='{}' kind='{}' seasons={}", it.id, it.title, it.year, it.kind, it.seasons);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_series_episode_streams() {
        let mut client = super::MovieBoxClient::new();
        // The Mentalist id: 7845473610491125400
        let id = "7845473610491125400";

        let s1e1 = client.series_streams(id, 1, 1).await.unwrap();
        println!("Streams found for S1 E1: count={}", s1e1.len());
        for s in &s1e1 {
            println!("  S1E1 Stream: label='{}' res={}", s.label, s.resolution);
        }
        assert!(s1e1.iter().any(|s| s.label.contains("Pilot")));

        let s2e3 = client.series_streams(id, 2, 3).await.unwrap();
        println!("Streams found for S2 E3: count={}", s2e3.len());
        for s in &s2e3 {
            println!("  S2E3 Stream: label='{}' res={}", s.label, s.resolution);
        }
        assert!(s2e3.iter().any(|s| s.label.contains("Red Badge")));

        let s7e13 = client.series_streams(id, 7, 13).await.unwrap();
        println!("Streams found for S7 E13: count={}", s7e13.len());
        for s in &s7e13 {
            println!("  S7E13 Stream: label='{}' res={}", s.label, s.resolution);
        }
        assert!(s7e13.iter().any(|s| s.label.contains("White Orchids")));
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




