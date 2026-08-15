# CosmiX

A Rust/Tauri Windows desktop interface for searching supported movie providers, viewing
movie or series information, selecting an episode or stream quality, and opening
the chosen stream in VLC.

## Providers

- **MovieBox**: the default provider, using the signed OneRoom/MovieBox API
  flow.
- **CircleFTP (BDIX)**: direct JSON API search, details, episode, and direct
  stream support. Only works if [CircleFTP](http://circleftp.net) is accessible from the user end.
- **4KHDHub**: HTML search/details extraction, episode parsing, HubCloud and
  HubDrive resolution, PixelDrain URL normalization, and a video preflight
  before a final URL is passed to VLC.

Choose the provider from the settings button in the top-right corner.

# Run it

## For Rust Devs
Clone the repo, then run:

```powershell
cargo run
```

To build the app:
```powershell
cargo tauri build
```

The app tries to launch `vlc` from your PATH. If VLC is installed elsewhere,
it won't work.

## For Normal Users
Download the installer from the [Releases](https://github.com/AymanZakir/CosmiX/releases) page.

Then install it.
