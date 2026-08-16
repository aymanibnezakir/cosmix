# CosmiX

<div align="center">

![CosmiX Banner](ui/favicon.png)

**A lightweight, blazing-fast desktop media streaming and download client powered by Rust & Tauri.**

[![Release](https://img.shields.io/github/v/release/AymanZakir/CosmiX?style=for-the-badge&color=f4c34f&labelColor=191a1e)](https://github.com/AymanZakir/CosmiX/releases)
[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange?style=for-the-badge&logo=rust&logoColor=white&labelColor=191a1e)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-24C8D8?style=for-the-badge&logo=tauri&logoColor=white&labelColor=191a1e)](https://tauri.app/)
[![Platform](https://img.shields.io/badge/Platform-Windows-0078D6?style=for-the-badge&logo=windows&logoColor=white&labelColor=191a1e)](https://github.com/AymanZakir/CosmiX)

</div>

---

## Overview

**CosmiX** is a desktop application that provides a unified, ad-free interface to search movies and TV series across various streaming providers, browse detailed media metadata, select audio tracks & resolutions, and either **stream directly in VLC Media Player** or **download files locally** using an integrated, persistent download manager.

Its biggest strength lies in **seamless provider unification** and **zero ad bloat**—aggregating completely different streaming sources under a single, cohesive interface while entirely bypassing web pop-ups, redirects, captchas, and trackers via native backend Rust scrapers.

Built natively in **Rust** with **Tauri v2**, CosmiX is extremely lightweight on system resources, starts up instantly, and features a dark-themed glassmorphism interface.

---

## Features

- **Unified Media Search** — Instant search across multiple providers with smart query filtering.
- **Direct VLC Streaming** — Launches streams straight into VLC Media Player with all required authentication headers and streaming preflights handled automatically.
- **Persistent Download Manager** — Download and manage media locally.
- **Multi-Audio & Dub Selector** — Choose between Original Audio and various dubbed audio tracks.
- **Full TV Series & Season Navigation** — Clean season-by-season episode grid selector.
- **Zero Ad Bloat** — Bypasses web redirects, pop-ups, and trackers directly via backend Rust scrapers.

---

## Supported Providers

CosmiX features a modular provider architecture. You can switch between providers at any time via the **Settings** menu in the top-right corner.

| Provider | Speed | Quality | Best For | Description |
| :--- | :---: | :---: | :--- | :--- |
| **MovieBox** *(Default)* | Fast | 1080p / 720p | General Movies & Series | Emulates signed Android client requests with HMAC-MD5 encryption, device profile tokens, multiple audio dub options, and clean title deduplication. |
| **BDIX – CircleFTP** | Gigabit | Direct / 1080p | BDIX / Local ISP Users | Direct JSON API integration with CircleFTP servers for high-speed local peering streaming in Bangladesh/BDIX networks. |
| **4KHDHub** | Moderate | 4K / 1080p Remux | Cinephiles & 4K Rips | Advanced scraper that resolves HubCloud, HubDrive, and PixelDrain mirrors, preflighting direct video streams with custom playback headers. |

---

## Planned Providers & Roadmap

CosmiX is continuously expanding its provider ecosystem. Future releases will include:

- [ ] **Additional BDIX / Local FTP Sources** — Expanding local peering options (e.g. SamOnline, RoarZone, ICC).
- [ ] **Debrid & Torrent Integration** — Real-Debrid / AllDebrid stream resolver for multi-source torrent caching.
- [ ] **Public Video Hosting Scrapers** — Integrations with popular community streaming backends.
- [ ] **Subtitles Integration** — Auto-fetching `.srt` subtitles directly into the media player.
- [ ] **Custom Video Player Selection** — Support for `mpv`, `MPC-HC`, and `PotPlayer` in addition to VLC.

---

## Getting Started

### Prerequisites

1. **VLC Media Player** must be installed on your system (located at standard install directories).
2. **Windows 10 / 11** (64-bit).

---

### Installation for Users

1. Go to the [Releases](https://github.com/AymanZakir/CosmiX/releases) page.
2. Download the latest installer.
3. Run the installer and launch **CosmiX**.

---

### Building from Source (For Developers)

Make sure you have [Rust](https://rustup.rs/) (2024 Edition / 1.85+) and [Node.js](https://nodejs.org/) (Optional) installed on your machine.

1. **Clone the repository:**
   ```bash
   git clone https://github.com/AymanZakir/CosmiX.git
   cd CosmiX
   ```

2. **Run in development mode:**
   ```powershell
   cargo tauri dev
   ```
   *(or `cargo run`)*

3. **Build the production release package:**
   ```powershell
   cargo tauri build
   ```
   The compiled installer and standalone executable will be located in `target/release/bundle/`.

---

## Downloads Directory

By default, downloaded movies and episodes are neatly organized in your user downloads directory:
```
C:\Users\<YourUsername>\Downloads\CosmiX\
```
Download history and progress state are stored in `downloads.json` within your user AppData directory, ensuring zero data loss across sessions.

---

## Contributing

Contributions, issues, and feature requests are welcome! Feel free to check the [issues page](https://github.com/AymanZakir/CosmiX/issues) if you want to contribute.

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

---

## Author

**Ayman Zakir**
- GitHub: [@aymanibnezakir](https://github.com/aymanibnezakir)

---

<div align="center">
  Crafted with ❤ by <a href="https://github.com/aymanibnezakir">Ayman Zakir</a>
</div>
