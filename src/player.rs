use std::process::Command;

use crate::{
    error::{AppError, Result},
    models::StreamHeader,
};

pub fn launch_vlc(url: &str, headers: &[StreamHeader]) -> Result<()> {
    let candidates = [
        r"C:\Program Files\VideoLAN\VLC\vlc.exe",
        r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
    ];

    for executable in candidates {
        let mut command = Command::new(executable);
        command.arg("--play-and-exit");
        for header in headers {
            match header.name.to_ascii_lowercase().as_str() {
                "referer" => {
                    command.arg(format!("--http-referrer={}", header.value));
                }
                "user-agent" => {
                    command.arg(format!("--http-user-agent={}", header.value));
                }
                _ => {}
            }
        }
        if command.arg(url).spawn().is_ok() {
            return Ok(());
        }
    }

    Err(AppError::Message(
        "VLC was not found. Install VLC in C:\\Program Files\\.".into(),
    ))
}
