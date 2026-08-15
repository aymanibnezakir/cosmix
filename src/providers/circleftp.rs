use reqwest::Client;
use serde_json::Value;

use crate::{
    error::{AppError, Result},
    models::{Details, Episode, MediaItem, Stream},
};

const API_BASE: &str = "http://new.circleftp.net:5000/api";
const UPLOADS_BASE: &str = "http://new.circleftp.net:5000/uploads";

pub async fn search(query: &str) -> Result<Vec<MediaItem>> {
    let response: Value = Client::new()
        .get(format!("{API_BASE}/posts"))
        .query(&[("searchTerm", query), ("order", "desc")])
        .send()
        .await?
        .json()
        .await?;
    let posts = response["posts"]
        .as_array()
        .or(response.as_array())
        .cloned()
        .unwrap_or_default();

    let query_lower = query.to_ascii_lowercase();
    let query_trimmed = query_lower.trim();

    Ok(posts
        .iter()
        .filter_map(media_item)
        .filter(|item| item.title.to_ascii_lowercase().contains(query_trimmed))
        .collect())
}

pub async fn details(id: &str) -> Result<Details> {
    let post = post(id).await?;
    let kind = post["type"].as_str().unwrap_or("movie").to_owned();
    let episodes =
        if kind == "series" {
            post["content"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .flat_map(|(season_index, season)| {
                    season["episodes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .map(move |(episode_index, episode)| Episode {
                            season: (season_index + 1) as u32,
                            episode: (episode_index + 1) as u32,
                            label: episode["title"].as_str().map(str::to_owned).unwrap_or_else(
                                || format!("S{:02} · E{:02}", season_index + 1, episode_index + 1),
                            ),
                        })
                })
                .collect()
        } else {
            Vec::new()
        };

    Ok(Details {
        id: id.to_owned(),
        title: post["title"]
            .as_str()
            .or(post["name"].as_str())
            .unwrap_or("Untitled")
            .to_owned(),
        kind,
        year: value_text(&post["year"]).unwrap_or_else(|| "—".into()),
        poster: post["image"].as_str().map(poster_url),
        synopsis: post["metaData"]
            .as_str()
            .unwrap_or("No description available.")
            .to_owned(),
        rating: post["quality"].as_str().unwrap_or("—").to_owned(),
        genres: post["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|category| category["name"].as_str().map(str::to_owned))
            .collect(),
        episodes,
        dubs: Vec::new(),
    })
}

pub async fn streams(id: &str, season: Option<u32>, episode: Option<u32>) -> Result<Vec<Stream>> {
    let post = post(id).await?;
    let link = match (season, episode) {
        (Some(season), Some(episode)) => post["content"][(season - 1) as usize]["episodes"]
            [(episode - 1) as usize]["link"]
            .as_str(),
        _ => post["content"].as_str(),
    }
    .ok_or_else(|| AppError::Message("No playable CircleFTP link was found.".into()))?;

    let size_bytes = probe_content_length(link).await;

    Ok(vec![Stream {
        id: id.to_owned(),
        label: "Direct stream".into(),
        resolution: normalized_quality(post["quality"].as_str()),
        url: link.to_owned(),
        headers: vec![],
        size_bytes,
    }])
}

async fn probe_content_length(url: &str) -> Option<u64> {
    let response = Client::new().head(url).send().await.ok()?;
    response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

async fn post(id: &str) -> Result<Value> {
    Ok(Client::new()
        .get(format!("{API_BASE}/posts/{id}"))
        .send()
        .await?
        .json()
        .await?)
}

fn media_item(post: &Value) -> Option<MediaItem> {
    Some(MediaItem {
        id: value_text(&post["_id"]).or_else(|| value_text(&post["id"]))?,
        title: post["title"]
            .as_str()
            .or(post["name"].as_str())
            .unwrap_or("Untitled")
            .to_owned(),
        kind: post["type"].as_str().unwrap_or("movie").to_owned(),
        year: value_text(&post["year"]).unwrap_or_else(|| "—".into()),
        poster: post["image"].as_str().map(poster_url),
        seasons: 0,
        rating: value_text(&post["imdbRating"])
            .or_else(|| value_text(&post["imdbRatingValue"]))
            .or_else(|| value_text(&post["imdb"]))
            .or_else(|| value_text(&post["rating"]))
            .filter(|r| r != "0" && r != "0.0" && r != "—" && !r.is_empty()),
    })
}

fn poster_url(filename: &str) -> String {
    format!("{UPLOADS_BASE}/{filename}")
}

fn value_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
}

fn normalized_quality(raw: Option<&str>) -> String {
    let raw = raw.unwrap_or_default().to_ascii_lowercase();
    if raw.contains("2160") || raw.contains("4k") {
        "2160p".into()
    } else if raw.contains("1080") {
        "1080p".into()
    } else if raw.contains("720") {
        "720p".into()
    } else if raw.contains("480") {
        "480p".into()
    } else {
        "Auto".into()
    }
}

#[cfg(test)]
mod tests {
    use super::normalized_quality;

    #[test]
    fn normalizes_circleftp_quality_labels() {
        assert_eq!(normalized_quality(Some("4K WEB-DL")), "2160p");
        assert_eq!(normalized_quality(Some("1080p")), "1080p");
    }

    #[test]
    fn filters_search_results_by_case_insensitive_keyword() {
        let items = vec![
            crate::models::MediaItem {
                id: "1".into(),
                title: "The Mentalist".into(),
                kind: "series".into(),
                year: "2008".into(),
                poster: None,
                seasons: 0,
                rating: None,
            },
            crate::models::MediaItem {
                id: "2".into(),
                title: "Mental (2012)".into(),
                kind: "movie".into(),
                year: "2012".into(),
                poster: None,
                seasons: 0,
                rating: None,
            },
            crate::models::MediaItem {
                id: "3".into(),
                title: "Sex and Death 101".into(),
                kind: "movie".into(),
                year: "2008".into(),
                poster: None,
                seasons: 0,
                rating: None,
            },
        ];

        let query = "mentalist";
        let query_trimmed = query.trim().to_ascii_lowercase();
        let filtered = items
            .into_iter()
            .filter(|item| item.title.to_ascii_lowercase().contains(&query_trimmed))
            .collect::<Vec<_>>();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "The Mentalist");
    }
}
