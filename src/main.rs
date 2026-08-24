#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod download;
mod error;
mod models;
mod player;
mod providers;
mod state;

use state::AppState;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(AppState::new())
        .setup(|app| {
            // Show the window after a brief delay so the WebView has time to
            // render the dark-background HTML, eliminating the white flash.
            let window = app.get_webview_window("main").expect("main window");
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = window.show();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::search,
            commands::get_details,
            commands::get_streams,
            commands::play_in_vlc,
            commands::start_download,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::remove_download,
            commands::retry_download,
            commands::get_downloads,
            commands::open_download_location
        ])
        .run(tauri::generate_context!())
        .expect("Tauri app error");
}
