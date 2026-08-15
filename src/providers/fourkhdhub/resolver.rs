use std::net::IpAddr;

use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

use crate::models::StreamHeader;

pub(super) async fn resolve_and_preflight(
    client: &Client,
    resolver_url: &str,
    headers: &[StreamHeader],
) -> Option<(String, Option<u64>)> {
    let candidates = if resolver_url.contains("hubcloud.") {
        resolve_hubcloud(client, resolver_url).await
    } else if resolver_url.contains("hubdrive.") {
        resolve_hubdrive(client, resolver_url).await
    } else {
        vec![resolver_url.to_owned()]
    };

    for candidate in candidates {
        if let Some((url, size_bytes)) = preflight(client, &candidate, headers).await {
            return Some((url, size_bytes));
        }
    }
    None
}

async fn resolve_hubcloud(client: &Client, drive_url: &str) -> Vec<String> {
    let Ok(first_page) = client
        .get(drive_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    else {
        return vec![];
    };
    let Ok(first_html) = first_page.text().await else {
        return vec![];
    };
    let second_url = {
        let document = Html::parse_document(&first_html);
        let download = Selector::parse("a#download").expect("static selector");
        document
            .select(&download)
            .next()
            .and_then(|link| link.value().attr("href"))
            .map(|url| absolute_url(url, drive_url))
    };
    let Some(second_url) = second_url else {
        return vec![];
    };
    let Ok(second_page) = client
        .get(second_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    else {
        return vec![];
    };
    let source_url = second_page.url().to_string();
    let Ok(second_html) = second_page.text().await else {
        return vec![];
    };
    candidate_urls(&second_html, &source_url)
}

async fn resolve_hubdrive(client: &Client, drive_url: &str) -> Vec<String> {
    let Ok(page) = client
        .get(drive_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    else {
        return vec![];
    };
    let source_url = page.url().to_string();
    let Ok(html) = page.text().await else {
        return vec![];
    };
    let hubcloud_url = {
        let document = Html::parse_document(&html);
        let links = Selector::parse("a[href]").expect("static selector");
        document
            .select(&links)
            .filter_map(|link| link.value().attr("href"))
            .map(|href| absolute_url(href, &source_url))
            .find(|href| {
                href.contains("hubcloud.")
                    && Url::parse(href)
                        .ok()
                        .is_some_and(|url| url.path().starts_with("/drive/"))
            })
    };
    match hubcloud_url {
        Some(url) => resolve_hubcloud(client, &url).await,
        None => vec![],
    }
}

fn candidate_urls(html: &str, source_url: &str) -> Vec<String> {
    let mut urls = script_urls(html);
    let document = Html::parse_document(html);
    let links = Selector::parse("a[href]").expect("static selector");
    urls.extend(
        document
            .select(&links)
            .filter_map(|link| link.value().attr("href"))
            .map(|href| absolute_url(href, source_url)),
    );
    urls = urls
        .into_iter()
        .filter_map(normalize_pixeldrain)
        .filter(|url| valid_playback_url(url))
        .collect();
    urls.sort_by_key(|url| candidate_priority(url));
    urls.dedup();
    urls
}

fn script_urls(html: &str) -> Vec<String> {
    let prefixes = [
        "https://pixeldrain.dev/",
        "https://pixeldrain.com/",
        "https://pixel.hubcloud.",
    ];
    let mut urls = Vec::new();
    for prefix in prefixes {
        let mut remaining = html;
        while let Some(offset) = remaining.find(prefix) {
            let candidate = &remaining[offset..];
            let end = candidate
                .find(|character: char| {
                    matches!(
                        character,
                        '\'' | '"' | '<' | '>' | '\\' | ' ' | '\t' | '\r' | '\n'
                    )
                })
                .unwrap_or(candidate.len());
            urls.push(candidate[..end].to_owned());
            remaining = &candidate[end..];
        }
    }
    urls
}

async fn preflight(
    client: &Client,
    raw_url: &str,
    headers: &[StreamHeader],
) -> Option<(String, Option<u64>)> {
    probe_url(client, raw_url, headers, 0).await
}

async fn probe_url(
    client: &Client,
    raw_url: &str,
    headers: &[StreamHeader],
    depth: usize,
) -> Option<(String, Option<u64>)> {
    if depth > 3 {
        return None;
    }
    let url = normalize_pixeldrain_ref(raw_url)?;
    if !valid_playback_url(&url) {
        return None;
    }
    let mut request = client
        .get(&url)
        .header("Range", "bytes=0-0")
        .header("Accept", "*/*")
        .header("Accept-Encoding", "identity");
    for header in headers {
        request = request.header(&header.name, &header.value);
    }
    let response = request.send().await.ok()?.error_for_status().ok()?;
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if ["text/html", "application/zip", "text/plain"]
        .iter()
        .any(|bad| content_type.starts_with(bad))
    {
        let link = Url::parse(&final_url)
            .ok()?
            .query_pairs()
            .find(|(key, _)| key == "link")?
            .1
            .into_owned();

        if let Some(res) = Box::pin(probe_url(client, &link, headers, depth + 1)).await {
            return Some(res);
        }
        return valid_playback_url(&link).then_some((link, None));
    }

    let size_bytes = parse_response_size(&response);
    valid_playback_url(&final_url).then_some((final_url, size_bytes))
}

fn parse_response_size(response: &reqwest::Response) -> Option<u64> {
    if let Some(range_header) = response.headers().get(reqwest::header::CONTENT_RANGE) {
        if let Ok(range_str) = range_header.to_str() {
            if let Some(size) = parse_content_range_total(range_str) {
                return Some(size);
            }
        }
    }
    if response.status() == reqwest::StatusCode::OK {
        if let Some(length_header) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
            if let Ok(length_str) = length_header.to_str() {
                if let Ok(size) = length_str.trim().parse::<u64>() {
                    return Some(size);
                }
            }
        }
    }
    None
}

fn parse_content_range_total(header_val: &str) -> Option<u64> {
    let (_, total) = header_val.rsplit_once('/')?;
    total.trim().parse::<u64>().ok()
}

fn normalize_pixeldrain(raw: String) -> Option<String> {
    normalize_pixeldrain_ref(&raw)
}
fn normalize_pixeldrain_ref(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?;
    if !host.contains("pixeldrain.") {
        return Some(raw.to_owned());
    }
    let id = url
        .path()
        .strip_prefix("/u/")
        .or_else(|| url.path().strip_prefix("/api/file/"))?
        .trim_matches('/');
    (!id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')))
    .then(|| format!("https://{host}/api/file/{id}?download"))
}

fn valid_playback_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".local")
        || url.path().ends_with(".zip")
        || url.path().contains("/login")
        || url.path().contains("/logout")
    {
        return false;
    }
    host.parse::<IpAddr>().map(public_ip).unwrap_or(true)
}

fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            !address.is_private()
                && !address.is_loopback()
                && !address.is_unspecified()
                && !address.is_link_local()
        }
        IpAddr::V6(address) => !address.is_loopback() && !address.is_unspecified(),
    }
}

fn candidate_priority(url: &str) -> u8 {
    let url = url.to_ascii_lowercase();
    if url.contains("pixeldrain") || url.contains("pixel.hubcloud") {
        0
    } else if url.contains("gpdl.") || url.contains("googleusercontent") {
        1
    } else if url.contains("workers.dev") || url.contains("r2.dev") {
        2
    } else if url.contains("latent.click") || url.contains("fsl") {
        3
    } else {
        4
    }
}

fn absolute_url(raw: &str, base: &str) -> String {
    Url::parse(base)
        .ok()
        .and_then(|base| base.join(raw).ok())
        .map(|url| url.into())
        .unwrap_or_else(|| raw.into())
}

#[cfg(test)]
mod tests {
    use super::{normalize_pixeldrain_ref, parse_content_range_total, valid_playback_url};
    #[test]
    fn normalizes_pixeldrain_share_url() {
        assert_eq!(
            normalize_pixeldrain_ref("https://pixeldrain.com/u/abc_123"),
            Some("https://pixeldrain.com/api/file/abc_123?download".into())
        );
    }
    #[test]
    fn rejects_local_playback_urls() {
        assert!(!valid_playback_url("https://127.0.0.1/video.mp4"));
    }
    #[test]
    fn parses_content_range_size() {
        assert_eq!(
            parse_content_range_total("bytes 0-0/5368709120"),
            Some(5368709120)
        );
        assert_eq!(
            parse_content_range_total("bytes 0-1023/1048576"),
            Some(1048576)
        );
        assert_eq!(parse_content_range_total("bytes */*"), None);
    }
}
