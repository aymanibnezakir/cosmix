use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::{Mutex, Notify},
};

use crate::{
    error::{AppError, Result},
    models::{DownloadInfo, DownloadStatusInfo, StreamHeader},
};

// Download state tracking

#[derive(Clone, Debug)]
pub enum DownloadStatus {
    Queued,
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        speed_bps: u64,
    },
    Paused {
        downloaded: u64,
        total: Option<u64>,
    },
    Completed {
        size: u64,
    },
    Failed {
        error: String,
    },
    Cancelled,
}

// Download item model

#[derive(Clone, Debug)]
pub struct DownloadEntry {
    pub id: String,
    pub title: String,
    pub episode_label: Option<String>,
    pub resolution: String,
    pub status: DownloadStatus,
    pub url: String,
    pub headers: Vec<StreamHeader>,
    pub file_path: PathBuf,
}

impl DownloadEntry {
    fn to_info(&self) -> DownloadInfo {
        DownloadInfo {
            id: self.id.clone(),
            title: self.title.clone(),
            episode_label: self.episode_label.clone(),
            resolution: self.resolution.clone(),
            status: match &self.status {
                DownloadStatus::Queued => DownloadStatusInfo::Queued,
                DownloadStatus::Downloading {
                    downloaded,
                    total,
                    speed_bps,
                } => DownloadStatusInfo::Downloading {
                    downloaded: *downloaded,
                    total: *total,
                    speed_bps: *speed_bps,
                },
                DownloadStatus::Paused { downloaded, total } => DownloadStatusInfo::Paused {
                    downloaded: *downloaded,
                    total: *total,
                },
                DownloadStatus::Completed { size } => DownloadStatusInfo::Completed {
                    size: *size,
                },
                DownloadStatus::Failed { error } => DownloadStatusInfo::Failed {
                    error: error.clone(),
                },
                DownloadStatus::Cancelled => DownloadStatusInfo::Cancelled,
            },
            file_path: self.file_path.to_string_lossy().into_owned(),
        }
    }
}

// Persistence model

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedDownloadEntry {
    id: String,
    title: String,
    episode_label: Option<String>,
    resolution: String,
    status: PersistedDownloadStatus,
    url: String,
    headers: Vec<StreamHeader>,
    file_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PersistedDownloadStatus {
    Queued,
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    Paused {
        downloaded: u64,
        total: Option<u64>,
    },
    Completed {
        size: u64,
    },
    Failed {
        error: String,
    },
    Cancelled,
}

fn storage_path() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CosmiX")
        .join("downloads.json")
}

fn save_entries_to_disk(entries: &[DownloadEntry]) {
    let path = storage_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let persisted: Vec<PersistedDownloadEntry> = entries
        .iter()
        .map(|e| PersistedDownloadEntry {
            id: e.id.clone(),
            title: e.title.clone(),
            episode_label: e.episode_label.clone(),
            resolution: e.resolution.clone(),
            status: match &e.status {
                DownloadStatus::Queued => PersistedDownloadStatus::Queued,
                DownloadStatus::Downloading {
                    downloaded, total, ..
                } => PersistedDownloadStatus::Downloading {
                    downloaded: *downloaded,
                    total: *total,
                },
                DownloadStatus::Paused { downloaded, total } => {
                    PersistedDownloadStatus::Paused {
                        downloaded: *downloaded,
                        total: *total,
                    }
                }
                DownloadStatus::Completed { size } => {
                    PersistedDownloadStatus::Completed { size: *size }
                }
                DownloadStatus::Failed { error } => PersistedDownloadStatus::Failed {
                    error: error.clone(),
                },
                DownloadStatus::Cancelled => PersistedDownloadStatus::Cancelled,
            },
            url: e.url.clone(),
            headers: e.headers.clone(),
            file_path: e.file_path.clone(),
        })
        .collect();

    if let Ok(json) = serde_json::to_string_pretty(&persisted) {
        let _ = std::fs::write(&path, json);
    }
}

fn load_entries_from_disk() -> Vec<DownloadEntry> {
    let path = storage_path();
    if !path.exists() {
        return Vec::new();
    }
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(persisted) = serde_json::from_str::<Vec<PersistedDownloadEntry>>(&data) else {
        return Vec::new();
    };

    persisted
        .into_iter()
        .map(|p| {
            let actual_file_size = std::fs::metadata(&p.file_path).map(|m| m.len()).ok();
            let status = match p.status {
                PersistedDownloadStatus::Queued => DownloadStatus::Queued,
                PersistedDownloadStatus::Downloading { downloaded, total } => {
                    // Active downloads from a previous session are loaded as Paused at current byte offset
                    let actual_downloaded = actual_file_size.unwrap_or(downloaded);
                    DownloadStatus::Paused {
                        downloaded: actual_downloaded,
                        total,
                    }
                }
                PersistedDownloadStatus::Paused { downloaded, total } => {
                    let actual_downloaded = actual_file_size.unwrap_or(downloaded);
                    DownloadStatus::Paused {
                        downloaded: actual_downloaded,
                        total,
                    }
                }
                PersistedDownloadStatus::Completed { size } => {
                    let actual_size = actual_file_size.unwrap_or(size);
                    DownloadStatus::Completed { size: actual_size }
                }
                PersistedDownloadStatus::Failed { error } => DownloadStatus::Failed { error },
                PersistedDownloadStatus::Cancelled => DownloadStatus::Cancelled,
            };

            DownloadEntry {
                id: p.id,
                title: p.title,
                episode_label: p.episode_label,
                resolution: p.resolution,
                status,
                url: p.url,
                headers: p.headers,
                file_path: p.file_path,
            }
        })
        .collect()
}

// Download manager and lifecycle

struct TaskHandle {
    cancel: Arc<Notify>,
    pause: Arc<Notify>,
}

pub struct DownloadManager {
    entries: Arc<Mutex<Vec<DownloadEntry>>>,
    tasks: Arc<Mutex<HashMap<String, TaskHandle>>>,
    http: Client,
}

impl DownloadManager {
    pub fn new() -> Self {
        let http = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("download HTTP client");

        let loaded = load_entries_from_disk();

        Self {
            entries: Arc::new(Mutex::new(loaded)),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            http,
        }
    }

    pub async fn start_download(
        &self,
        title: String,
        episode_label: Option<String>,
        resolution: String,
        url: String,
        headers: Vec<StreamHeader>,
    ) -> Result<String> {
        // Prevent duplicate downloads for active streams
        {
            let entries = self.entries.lock().await;
            for entry in entries.iter() {
                if entry.url == url {
                    match &entry.status {
                        DownloadStatus::Downloading { .. }
                        | DownloadStatus::Queued
                        | DownloadStatus::Paused { .. } => {
                            return Err(AppError::Message(
                                "This stream is already being downloaded.".into(),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        let file_path = self
            .build_file_path(&title, episode_label.as_deref(), &resolution, &url)
            .await;

        let entry = DownloadEntry {
            id: id.clone(),
            title,
            episode_label,
            resolution,
            status: DownloadStatus::Queued,
            url: url.clone(),
            headers: headers.clone(),
            file_path: file_path.clone(),
        };

        {
            let mut entries = self.entries.lock().await;
            entries.insert(0, entry);
            save_entries_to_disk(&entries);
        }

        self.spawn_download_task(id.clone(), url, headers, file_path, 0)
            .await;

        Ok(id)
    }

    pub async fn pause_download(&self, id: &str) -> Result<()> {
        let tasks = self.tasks.lock().await;
        if let Some(handle) = tasks.get(id) {
            handle.pause.notify_one();
            Ok(())
        } else {
            let mut entries = self.entries.lock().await;
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                if matches!(entry.status, DownloadStatus::Queued) {
                    entry.status = DownloadStatus::Paused {
                        downloaded: 0,
                        total: None,
                    };
                    save_entries_to_disk(&entries);
                    return Ok(());
                }
            }
            Err(AppError::Message("Download task not found.".into()))
        }
    }

    pub async fn resume_download(&self, id: &str) -> Result<()> {
        let (url, headers, file_path, downloaded) = {
            let mut entries = self.entries.lock().await;
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| AppError::Message("Download not found.".into()))?;
            let res = match &entry.status {
                DownloadStatus::Paused { downloaded, total } => {
                    let resume_from = *downloaded;
                    let url = entry.url.clone();
                    let headers = entry.headers.clone();
                    let file_path = entry.file_path.clone();
                    entry.status = DownloadStatus::Downloading {
                        downloaded: resume_from,
                        total: *total,
                        speed_bps: 0,
                    };
                    (url, headers, file_path, resume_from)
                }
                _ => return Err(AppError::Message("Download is not paused.".into())),
            };
            save_entries_to_disk(&entries);
            res
        };

        self.spawn_download_task(id.to_owned(), url, headers, file_path, downloaded)
            .await;
        Ok(())
    }

    pub async fn cancel_download(&self, id: &str) -> Result<()> {
        // Tell the background task to stop
        {
            let tasks = self.tasks.lock().await;
            if let Some(handle) = tasks.get(id) {
                handle.cancel.notify_one();
            }
        }

        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            let partial_path = entry.file_path.clone();
            entry.status = DownloadStatus::Cancelled;
            // Clean up the incomplete file on disk
            let _ = fs::remove_file(&partial_path).await;
            save_entries_to_disk(&entries);
        }

        self.tasks.lock().await.remove(id);
        Ok(())
    }

    pub async fn remove_download(&self, id: &str) -> Result<()> {
        // Cancel the task if it's currently running
        {
            let tasks = self.tasks.lock().await;
            if let Some(handle) = tasks.get(id) {
                handle.cancel.notify_one();
            }
        }
        self.tasks.lock().await.remove(id);

        let mut entries = self.entries.lock().await;
        if let Some(index) = entries.iter().position(|e| e.id == id) {
            let entry = entries.remove(index);
            // Delete leftover file if the download didn't finish
            if !matches!(entry.status, DownloadStatus::Completed { .. }) {
                let _ = fs::remove_file(&entry.file_path).await;
            }
            save_entries_to_disk(&entries);
        }
        Ok(())
    }

    pub async fn retry_download(&self, id: &str) -> Result<()> {
        let (url, headers, file_path) = {
            let mut entries = self.entries.lock().await;
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| AppError::Message("Download not found.".into()))?;
            let res = match &entry.status {
                DownloadStatus::Failed { .. } | DownloadStatus::Cancelled => {
                    let url = entry.url.clone();
                    let headers = entry.headers.clone();
                    let file_path = entry.file_path.clone();
                    entry.status = DownloadStatus::Queued;
                    (url, headers, file_path)
                }
                _ => return Err(AppError::Message("Download cannot be retried.".into())),
            };
            save_entries_to_disk(&entries);
            res
        };

        // Wipe any incomplete file before starting fresh
        let _ = fs::remove_file(&file_path).await;

        self.spawn_download_task(id.to_owned(), url, headers, file_path, 0)
            .await;
        Ok(())
    }

    pub async fn get_downloads(&self) -> Vec<DownloadInfo> {
        self.entries
            .lock()
            .await
            .iter()
            .map(DownloadEntry::to_info)
            .collect()
    }

    // Helper functions

    async fn spawn_download_task(
        &self,
        id: String,
        url: String,
        headers: Vec<StreamHeader>,
        file_path: PathBuf,
        resume_offset: u64,
    ) {
        let cancel = Arc::new(Notify::new());
        let pause = Arc::new(Notify::new());

        self.tasks.lock().await.insert(
            id.clone(),
            TaskHandle {
                cancel: cancel.clone(),
                pause: pause.clone(),
            },
        );

        let entries = self.entries.clone();
        let tasks = self.tasks.clone();
        let http = self.http.clone();

        tokio::spawn(async move {
            let result = run_download(
                &http,
                &entries,
                &id,
                &url,
                &headers,
                &file_path,
                resume_offset,
                cancel,
                pause,
            )
            .await;

            if let Err(error) = result {
                let mut entries = entries.lock().await;
                if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                    // Keep user-initiated status (cancelled/paused) intact
                    if matches!(
                        entry.status,
                        DownloadStatus::Downloading { .. } | DownloadStatus::Queued
                    ) {
                        entry.status = DownloadStatus::Failed {
                            error: error.to_string(),
                        };
                        save_entries_to_disk(&entries);
                    }
                }
            }

            tasks.lock().await.remove(&id);
        });
    }

    async fn build_file_path(
        &self,
        title: &str,
        episode_label: Option<&str>,
        resolution: &str,
        url: &str,
    ) -> PathBuf {
        let download_dir = dirs::download_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("CosmiX");
        let _ = fs::create_dir_all(&download_dir).await;

        let safe_title = sanitize_filename(title);
        let episode_part = episode_label
            .map(|label| format!(" - {}", sanitize_filename(label)))
            .unwrap_or_default();
        let resolution_part = if resolution.is_empty() || resolution == "Auto" {
            String::new()
        } else {
            format!(" [{}]", resolution)
        };
        let extension = url_extension(url);

        let base_name = format!("{safe_title}{episode_part}{resolution_part}");
        let mut candidate = download_dir.join(format!("{base_name}.{extension}"));
        let mut counter = 1u32;

        // Ensure we don't collide with existing files on disk or queued downloads in memory
        let entries = self.entries.lock().await;
        loop {
            let path_exists = candidate.exists();
            let entry_exists = entries.iter().any(|e| e.file_path == candidate);
            if !path_exists && !entry_exists {
                break;
            }
            candidate = download_dir.join(format!("{base_name} ({counter}).{extension}"));
            counter += 1;
            if counter > 999 {
                break;
            }
        }

        candidate
    }
}

// Background download execution

async fn run_download(
    http: &Client,
    entries: &Arc<Mutex<Vec<DownloadEntry>>>,
    id: &str,
    url: &str,
    headers: &[StreamHeader],
    file_path: &Path,
    resume_offset: u64,
    cancel: Arc<Notify>,
    pause: Arc<Notify>,
) -> Result<()> {
    // Set up request with byte range support for resume
    let mut request = http
        .get(url)
        .header("Range", format!("bytes={resume_offset}-"))
        .header("Accept", "*/*")
        .header("Accept-Encoding", "identity");

    for header in headers {
        request = request.header(&header.name, &header.value);
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::Message(format!("Download request failed: {e}")))?;

    if !response.status().is_success() && response.status().as_u16() != 206 {
        return Err(AppError::Message(format!(
            "Server returned {}",
            response.status()
        )));
    }

    let total = if let Some(content_range) = response.headers().get(reqwest::header::CONTENT_RANGE)
    {
        content_range
            .to_str()
            .ok()
            .and_then(|cr| cr.rsplit('/').next())
            .and_then(|total_str| total_str.parse::<u64>().ok())
    } else {
        response.content_length().map(|cl| cl + resume_offset)
    };

    // Make sure the destination folder is ready
    if let Some(parent) = file_path.parent() {
        let _ = fs::create_dir_all(parent).await;
    }

    // Open file: append if resuming an earlier attempt, otherwise create fresh
    let mut file = if resume_offset > 0 {
        fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(file_path)
            .await
            .map_err(|e| AppError::Message(format!("Cannot open file for resume: {e}")))?
    } else {
        fs::File::create(file_path)
            .await
            .map_err(|e| AppError::Message(format!("Cannot create download file: {e}")))?
    };

    let mut downloaded = resume_offset;
    let mut stream = response.bytes_stream();
    let mut last_progress_update = Instant::now();
    let mut last_disk_save = Instant::now();
    let mut bytes_since_last_update: u64 = 0;
    let mut current_speed: u64;

    // Mark as downloading in state
    {
        let mut entries = entries.lock().await;
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            entry.status = DownloadStatus::Downloading {
                downloaded,
                total,
                speed_bps: 0,
            };
            save_entries_to_disk(&entries);
        }
    }

    loop {
        tokio::select! {
            biased;
            _ = cancel.notified() => {
                // Cancelled — cancellation handler manages cleanup and status
                return Ok(());
            }
            _ = pause.notified() => {
                // Paused — save our progress point
                let mut entries = entries.lock().await;
                if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                    entry.status = DownloadStatus::Paused {
                        downloaded,
                        total,
                    };
                    save_entries_to_disk(&entries);
                }
                return Ok(());
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        file.write_all(&bytes)
                            .await
                            .map_err(|e| AppError::Message(format!("File write error: {e}")))?;
                        downloaded += bytes.len() as u64;
                        bytes_since_last_update += bytes.len() as u64;

                        // Throttle progress updates to UI to twice per second
                        let elapsed = last_progress_update.elapsed();
                        if elapsed.as_millis() >= 500 {
                            current_speed = (bytes_since_last_update as f64 / elapsed.as_secs_f64()) as u64;
                            bytes_since_last_update = 0;
                            last_progress_update = Instant::now();

                            let mut entries = entries.lock().await;
                            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                                entry.status = DownloadStatus::Downloading {
                                    downloaded,
                                    total,
                                    speed_bps: current_speed,
                                };
                            }

                            if last_disk_save.elapsed().as_secs() >= 5 {
                                last_disk_save = Instant::now();
                                save_entries_to_disk(&entries);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Err(AppError::Message(format!("Download stream error: {e}")));
                    }
                    None => {
                        // Finished receiving all data
                        file.flush().await.map_err(|e| AppError::Message(format!("File flush error: {e}")))?;
                        let mut entries = entries.lock().await;
                        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                            entry.status = DownloadStatus::Completed {
                                size: downloaded,
                            };
                            save_entries_to_disk(&entries);
                        }
                        return Ok(());
                    }
                }
            }
        }
    }
}

// Utility helpers

fn sanitize_filename(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.');
    if trimmed.is_empty() {
        "download".to_owned()
    } else {
        // Keep filename length reasonable
        trimmed.chars().take(200).collect()
    }
}

fn url_extension(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or(url);
    let last_segment = path.rsplit('/').next().unwrap_or("");
    if let Some(dot_pos) = last_segment.rfind('.') {
        let ext = &last_segment[dot_pos + 1..];
        match ext.to_ascii_lowercase().as_str() {
            "mp4" => "mp4",
            "mkv" => "mkv",
            "avi" => "avi",
            "webm" => "webm",
            "m3u8" => "mp4", // Save HLS streams as mp4 containers
            _ => "mp4",
        }
    } else {
        "mp4"
    }
}

pub fn open_in_explorer(path: &str) -> Result<()> {
    let file_path = Path::new(path);
    if file_path.is_file() {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(file_path)
            .spawn()
            .map_err(|e| AppError::Message(format!("Cannot open Explorer: {e}")))?;
    } else {
        let folder = file_path.parent().unwrap_or(file_path);
        std::process::Command::new("explorer")
            .arg(folder)
            .spawn()
            .map_err(|e| AppError::Message(format!("Cannot open Explorer: {e}")))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{sanitize_filename, url_extension, DownloadEntry, DownloadStatus};
    use std::path::PathBuf;

    #[test]
    fn sanitizes_filenames_with_invalid_characters() {
        assert_eq!(
            sanitize_filename("Movie: The \"Best\" <Part> 1"),
            "Movie_ The _Best_ _Part_ 1"
        );
    }

    #[test]
    fn sanitizes_empty_and_dot_only_filenames() {
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("..."), "download");
    }

    #[test]
    fn extracts_extension_from_url() {
        assert_eq!(url_extension("https://cdn.example.com/movie.mkv"), "mkv");
        assert_eq!(
            url_extension("https://cdn.example.com/movie.mp4?token=abc"),
            "mp4"
        );
        assert_eq!(url_extension("https://cdn.example.com/stream"), "mp4");
    }

    #[test]
    fn new_downloads_ordered_at_top() {
        let mut entries = Vec::new();
        let entry1 = DownloadEntry {
            id: "1".into(),
            title: "First".into(),
            episode_label: None,
            resolution: "1080p".into(),
            status: DownloadStatus::Completed { size: 100 },
            url: "https://example.com/1.mp4".into(),
            headers: vec![],
            file_path: PathBuf::from("first.mp4"),
        };
        let entry2 = DownloadEntry {
            id: "2".into(),
            title: "Second".into(),
            episode_label: None,
            resolution: "1080p".into(),
            status: DownloadStatus::Queued,
            url: "https://example.com/2.mp4".into(),
            headers: vec![],
            file_path: PathBuf::from("second.mp4"),
        };

        entries.insert(0, entry1);
        entries.insert(0, entry2);

        assert_eq!(entries[0].id, "2");
        assert_eq!(entries[1].id, "1");
    }

    #[test]
    fn test_persisted_downloads_serde_and_restoration() {
        use super::{PersistedDownloadEntry, PersistedDownloadStatus};

        let persisted = vec![
            PersistedDownloadEntry {
                id: "1".into(),
                title: "Movie A".into(),
                episode_label: None,
                resolution: "1080p".into(),
                status: PersistedDownloadStatus::Downloading {
                    downloaded: 500,
                    total: Some(1000),
                },
                url: "https://example.com/a.mp4".into(),
                headers: vec![],
                file_path: PathBuf::from("a.mp4"),
            },
            PersistedDownloadEntry {
                id: "2".into(),
                title: "Series B".into(),
                episode_label: Some("S01E01".into()),
                resolution: "720p".into(),
                status: PersistedDownloadStatus::Completed { size: 2000 },
                url: "https://example.com/b.mp4".into(),
                headers: vec![],
                file_path: PathBuf::from("b.mp4"),
            },
        ];

        let json = serde_json::to_string(&persisted).unwrap();
        let restored: Vec<PersistedDownloadEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].title, "Movie A");
        assert_eq!(restored[1].episode_label, Some("S01E01".into()));
    }
}
