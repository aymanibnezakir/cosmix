use tauri::State;

use crate::{
    download,
    error::Result,
    models::{Details, DownloadInfo, MediaItem, Provider, Stream, StreamHeader},
    player::launch_vlc,
    providers::circleftp,
    state::AppState,
};

#[tauri::command]
pub async fn search(
    provider: Provider,
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<MediaItem>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    match provider {
        Provider::Moviebox => state.moviebox.lock().await.search(query.trim()).await,
        Provider::Fourkhdhub => crate::providers::fourkhdhub::search(query.trim()).await,
        Provider::Circleftp => circleftp::search(query.trim()).await,
    }
}

#[tauri::command]
pub async fn get_details(
    provider: Provider,
    id: String,
    state: State<'_, AppState>,
) -> Result<Details> {
    match provider {
        Provider::Moviebox => state.moviebox.lock().await.details(&id).await,
        Provider::Fourkhdhub => crate::providers::fourkhdhub::details(&id).await,
        Provider::Circleftp => circleftp::details(&id).await,
    }
}

#[tauri::command]
pub async fn get_streams(
    provider: Provider,
    id: String,
    season: Option<u32>,
    episode: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<Stream>> {
    match provider {
        Provider::Moviebox => {
            state
                .moviebox
                .lock()
                .await
                .streams(&id, season, episode)
                .await
        }
        Provider::Fourkhdhub => crate::providers::fourkhdhub::streams(&id, season, episode).await,
        Provider::Circleftp => circleftp::streams(&id, season, episode).await,
    }
}

#[tauri::command]
pub fn play_in_vlc(url: String, headers: Option<Vec<StreamHeader>>) -> Result<()> {
    launch_vlc(&url, &headers.unwrap_or_default())
}

// ── Download commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_download(
    title: String,
    episode_label: Option<String>,
    resolution: String,
    url: String,
    headers: Option<Vec<StreamHeader>>,
    state: State<'_, AppState>,
) -> Result<String> {
    state
        .downloads
        .start_download(title, episode_label, resolution, url, headers.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn pause_download(id: String, state: State<'_, AppState>) -> Result<()> {
    state.downloads.pause_download(&id).await
}

#[tauri::command]
pub async fn resume_download(id: String, state: State<'_, AppState>) -> Result<()> {
    state.downloads.resume_download(&id).await
}

#[tauri::command]
pub async fn cancel_download(id: String, state: State<'_, AppState>) -> Result<()> {
    state.downloads.cancel_download(&id).await
}

#[tauri::command]
pub async fn remove_download(id: String, state: State<'_, AppState>) -> Result<()> {
    state.downloads.remove_download(&id).await
}

#[tauri::command]
pub async fn retry_download(id: String, state: State<'_, AppState>) -> Result<()> {
    state.downloads.retry_download(&id).await
}

#[tauri::command]
pub async fn get_downloads(state: State<'_, AppState>) -> Result<Vec<DownloadInfo>> {
    Ok(state.downloads.get_downloads().await)
}

#[tauri::command]
pub fn open_download_location(path: String) -> Result<()> {
    download::open_in_explorer(&path)
}

