use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Selector};

use crate::models::{Details, Episode, MediaItem};

#[derive(Clone, Debug)]
pub(super) struct ReleaseCandidate {
    pub label: String,
    pub resolution: String,
    pub resolver_url: String,
}

pub(super) fn search_results(html: &str, base_url: &str) -> Vec<MediaItem> {
    let document = Html::parse_document(html);
    let cards = selector("a.movie-card");
    let title = selector(".movie-card-title");
    let meta = selector(".movie-card-meta");
    let image = selector("img");

    document
        .select(&cards)
        .filter_map(|card| {
            let href = card.value().attr("href")?;
            let title = text_of(card.select(&title).next()?)?;
            let meta_text = card
                .select(&meta)
                .next()
                .and_then(text_of)
                .unwrap_or_default();
            Some(MediaItem {
                id: path_or_url(href, base_url),
                title,
                kind: if href.contains("-series-") {
                    "series"
                } else {
                    "movie"
                }
                .into(),
                year: year_in(&meta_text).unwrap_or_else(|| "—".into()),
                poster: card
                    .select(&image)
                    .next()
                    .and_then(|image| image.value().attr("src"))
                    .map(|src| absolute_url(src, base_url)),
                seasons: season_count(&meta_text),
            })
        })
        .collect()
}

pub(super) fn details(html: &str, id: &str, base_url: &str) -> Details {
    let document = Html::parse_document(html);
    let title = document
        .select(&selector("h1"))
        .next()
        .and_then(text_of)
        .or_else(|| meta_content(&document, "meta[property=\"og:title\"]"))
        .unwrap_or_else(|| "Untitled".into());
    let description = document
        .select(&selector(".content-section p.mt-4"))
        .next()
        .and_then(text_of)
        .or_else(|| meta_content(&document, "meta[name=\"description\"]"))
        .unwrap_or_else(|| "No description available.".into());
    let poster = meta_content(&document, "meta[property=\"og:image\"]")
        .map(|url| absolute_url(&url, base_url));
    let rating = document
        .select(&selector(".imdb-score"))
        .next()
        .and_then(text_of)
        .unwrap_or_else(|| "—".into());
    let genres = document
        .select(&selector(".badge-outline a"))
        .filter_map(text_of)
        .filter(|genre| is_genre(genre))
        .collect::<Vec<_>>();
    let episodes = episodes(&document);
    let release_text = metadata(&document, "Release:")
        .or_else(|| metadata(&document, "Last Air:"))
        .unwrap_or_default();

    Details {
        id: id.into(),
        title: strip_trailing_year(&title),
        kind: if episodes.is_empty() {
            "movie"
        } else {
            "series"
        }
        .into(),
        year: year_in(&release_text)
            .unwrap_or_else(|| year_in(&title).unwrap_or_else(|| "—".into())),
        poster,
        synopsis: description,
        rating,
        genres,
        episodes,
    }
}

pub(super) fn releases(
    html: &str,
    base_url: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> Vec<ReleaseCandidate> {
    let document = Html::parse_document(html);
    let blocks = match (season, episode) {
        (Some(season), Some(episode)) => document
            .select(&selector("#episodes .episode-download-item"))
            .filter(|block| {
                block
                    .select(&selector(".episode-file-title"))
                    .next()
                    .and_then(text_of)
                    .and_then(|name| season_episode(&name))
                    == Some((season, episode))
            })
            .collect::<Vec<_>>(),
        _ => document
            .select(&selector(".download-item"))
            .collect::<Vec<_>>(),
    };

    let mut releases = Vec::new();
    for block in blocks {
        let filename = block
            .select(&selector(".episode-file-title, .file-title"))
            .next()
            .and_then(text_of)
            .unwrap_or_else(|| "Stream".into());
        let resolution = quality(&filename);
        for link in block.select(&selector("a[href]")) {
            let Some(href) = link.value().attr("href") else {
                continue;
            };
            let resolver_url = absolute_url(href, base_url);
            if !resolver_url.starts_with("https://") {
                continue;
            }
            let host = resolver_url.to_ascii_lowercase();
            if !(host.contains("hubcloud.")
                || host.contains("hubdrive.")
                || is_direct_link(&resolver_url))
            {
                continue;
            }
            let link_label = text_of(link).unwrap_or_default();
            releases.push(ReleaseCandidate {
                label: if link_label.is_empty() {
                    filename.clone()
                } else {
                    format!("{filename} — {link_label}")
                },
                resolution: resolution.clone(),
                resolver_url,
            });
        }
    }
    releases
}

pub(super) fn season_episode(filename: &str) -> Option<(u32, u32)> {
    let upper = filename.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    for index in 0..bytes.len().saturating_sub(4) {
        if bytes[index] != b'S' {
            continue;
        }
        let Some((season, next)) = digits(bytes, index + 1) else {
            continue;
        };
        if bytes.get(next) != Some(&b'E') {
            continue;
        }
        let Some((episode, _)) = digits(bytes, next + 1) else {
            continue;
        };
        return Some((season, episode));
    }
    None
}

fn digits(bytes: &[u8], start: usize) -> Option<(u32, usize)> {
    let end = bytes[start..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .map(|offset| start + offset)
        .unwrap_or(bytes.len());
    if end == start {
        return None;
    }
    let number = std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()?;
    Some((number, end))
}

fn episodes(document: &Html) -> Vec<Episode> {
    let mut seasons: BTreeMap<u32, BTreeMap<u32, Episode>> = BTreeMap::new();
    for block in document.select(&selector("#episodes .episode-download-item")) {
        let Some(filename) = block
            .select(&selector(".episode-file-title"))
            .next()
            .and_then(text_of)
        else {
            continue;
        };
        let Some((season, episode)) = season_episode(&filename) else {
            continue;
        };
        seasons.entry(season).or_default().insert(
            episode,
            Episode {
                season,
                episode,
                label: format!("S{season:02} · E{episode:02}"),
            },
        );
    }
    seasons
        .into_values()
        .flat_map(|episodes| episodes.into_values())
        .collect()
}

fn metadata(document: &Html, desired_label: &str) -> Option<String> {
    document
        .select(&selector(".metadata-item"))
        .find_map(|item| {
            let label = item
                .select(&selector(".metadata-label"))
                .next()
                .and_then(text_of)?;
            (label.trim().eq_ignore_ascii_case(desired_label))
                .then(|| {
                    item.select(&selector(".metadata-value"))
                        .next()
                        .and_then(text_of)
                })
                .flatten()
        })
}

fn quality(filename: &str) -> String {
    let name = filename.to_ascii_lowercase();
    for quality in ["2160p", "1080p", "720p", "480p"] {
        if name.contains(quality) {
            return quality.into();
        }
    }
    "Auto".into()
}

fn is_direct_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    [
        "pixeldrain.",
        "googleusercontent.",
        "workers.dev",
        "r2.dev",
        ".mkv",
        ".mp4",
        ".m3u8",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("static CSS selector")
}
fn text_of(element: ElementRef<'_>) -> Option<String> {
    Some(element.text().collect::<String>().trim().to_owned()).filter(|text| !text.is_empty())
}
fn meta_content(document: &Html, selector_text: &str) -> Option<String> {
    document
        .select(&selector(selector_text))
        .next()?
        .value()
        .attr("content")
        .map(str::to_owned)
}
fn absolute_url(raw: &str, base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .and_then(|base| base.join(raw).ok())
        .map(|url| url.into())
        .unwrap_or_else(|| raw.into())
}
fn path_or_url(raw: &str, base_url: &str) -> String {
    url::Url::parse(raw)
        .ok()
        .and_then(|url| (url.host_str().is_some()).then(|| url.path().to_owned()))
        .unwrap_or_else(|| absolute_url(raw, base_url))
}
fn year_in(text: &str) -> Option<String> {
    text.as_bytes()
        .windows(4)
        .find(|value| value.iter().all(u8::is_ascii_digit))
        .and_then(|value| std::str::from_utf8(value).ok())
        .map(str::to_owned)
}
fn season_count(text: &str) -> u32 {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter_map(|part| part.strip_prefix('S'))
        .filter_map(|part| part.parse().ok())
        .max()
        .unwrap_or(0)
}
fn strip_trailing_year(title: &str) -> String {
    title
        .strip_suffix(')')
        .and_then(|prefix| {
            prefix
                .rsplit_once('(')
                .filter(|(_, year)| year.len() == 4 && year.chars().all(char::is_numeric))
                .map(|(name, _)| name.trim().to_owned())
        })
        .unwrap_or_else(|| title.to_owned())
}
fn is_genre(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "action"
            | "adventure"
            | "animation"
            | "comedy"
            | "crime"
            | "documentary"
            | "drama"
            | "family"
            | "fantasy"
            | "history"
            | "horror"
            | "music"
            | "mystery"
            | "romance"
            | "science fiction"
            | "sci-fi"
            | "thriller"
            | "tv movie"
            | "war"
            | "western"
    )
}

#[cfg(test)]
mod tests {
    use super::season_episode;
    #[test]
    fn reads_episode_identifiers_from_release_names() {
        assert_eq!(season_episode("Show S02E11 1080p"), Some((2, 11)));
    }
}
