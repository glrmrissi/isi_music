# isi_music

[![Release](https://img.shields.io/github/v/release/glrmrissi/isi_music?style=flat-square&color=1DB954&label=version)](https://github.com/glrmrissi/isi_music/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/glrmrissi/isi_music/release.yml?style=flat-square&label=build)](https://github.com/glrmrissi/isi_music/actions/workflows/release.yml)
[![License](https://img.shields.io/github/license/glrmrissi/isi_music?style=flat-square)](LICENSE)

A terminal-based Spotify player written in Rust. Uses librespot for native audio playback — no browser, no Electron, just your terminal.

<img width="1915" height="1031" alt="image" src="https://github.com/user-attachments/assets/ff4de1b1-488f-4d9c-8e8f-411c7990c0c7" />

## Features

- Native audio playback via librespot (no Spotify app required)
- Full-text search across tracks, albums, artists, playlists, and podcasts
- Queue management with drag-free ordering
- Shuffle and repeat modes (off / queue / track)
- Album art rendered with half-block characters in the terminal
- Audio visualizer
- Last.fm scrobbling
- Keyboard-driven interface

> **Requires Spotify Premium** — librespot needs a Premium account for audio streaming.

---

## Installation

### Linux

**1. Download the binary**

Go to the [Releases page](https://github.com/glrmrissi/isi_music/releases/latest) and download `isi-music-linux`.

```bash
chmod +x isi-music-linux
sudo mv isi-music-linux /usr/local/bin/isi-music
```

**2. Install audio dependencies**

On Debian/Ubuntu:
```bash
sudo apt install libasound2 libpulse0
```

On Arch Linux:
```bash
sudo pacman -S alsa-lib libpulse
```

On Fedora:
```bash
sudo dnf install alsa-lib pulseaudio-libs
```

**3. Run**
```bash
isi-music
```

---

### Windows

**1. Download the binary**

Go to the [Releases page](https://github.com/glrmrissi/isi_music/releases/latest) and download `isi-music-windows.exe`.

**2. No additional dependencies needed** — audio uses WASAPI, which is built into Windows.

**3. Run**

Open a terminal (Windows Terminal recommended for best rendering):
```powershell
.\isi-music-windows.exe
```

> **Note:** For proper album art and UI rendering, use a terminal with true color and a font that supports Unicode block characters (e.g. [Windows Terminal](https://aka.ms/terminal) + [Nerd Font](https://www.nerdfonts.com/)).

---

### Build from source

You need Rust 1.85+ (edition 2024).

**Linux:**
```bash
sudo apt install libasound2-dev libpulse-dev libdbus-1-dev  # Debian/Ubuntu
git clone https://github.com/glrmrissi/isi_music.git
cd isi_music
cargo build --release
./target/release/isi-music
```

**Windows (MSVC):**
```powershell
git clone https://github.com/glrmrissi/isi_music.git
cd isi_music
cargo build --release
.\target\release\isi-music.exe
```

---

## Configuration

On first run, isi_music will open a browser window for Spotify OAuth authentication. After authorizing, the token is cached locally and reused on subsequent runs.

The config file is created automatically at:
- **Linux:** `~/.config/isi-music/config.toml`
- **Windows:** `%APPDATA%\isi-music\config.toml`

You need to register a Spotify app at [developer.spotify.com](https://developer.spotify.com/dashboard) and add your credentials:

```toml
[spotify]
client_id     = "your_client_id_here"
client_secret = "your_client_secret_here"

# Optional: Last.fm scrobbling
[lastfm]
api_key    = "your_lastfm_api_key"
api_secret = "your_lastfm_api_secret"
username   = "your_lastfm_username"
password   = "your_lastfm_password"
```

In your Spotify app dashboard, set the redirect URI to:
```
http://127.0.0.1:8888/callback
```

---

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` | Cycle between panels |
| `Enter` | Play selected / confirm |
| `Space` | Play / pause |
| `n` | Next track |
| `p` | Previous track |
| `s` | Toggle shuffle |
| `r` | Cycle repeat mode |
| `+` / `-` | Volume up / down |
| `/` | Focus search |
| `q` | Add to queue |
| `Esc` | Back / cancel |
| `Ctrl+C` | Quit |

---

## How it works

isi_music combines two Spotify integrations:

- **librespot** — an open-source Spotify Connect client that handles authentication and streams audio directly to your system's audio output. This is what plays the music.
- **rspotify** — the Spotify Web API client used for search, metadata, album art, and queue information.

The TUI is built with [ratatui](https://github.com/ratatui-org/ratatui). All state is managed in a single app loop that polls player events, reacts to keyboard input, and re-renders the UI at ~30fps.

```
┌──────────────┐     OAuth      ┌─────────────────┐
│   isi_music  │ ────────────► │  Spotify Web API │  (search, metadata)
│              │ ◄──────────── │                  │
│              │                └─────────────────┘
│              │   librespot    ┌─────────────────┐
│              │ ────────────► │  Spotify servers │  (audio stream)
│              │ ◄──────────── │                  │
└──────────────┘                └─────────────────┘
       │
       ▼
  System audio (ALSA / PulseAudio / WASAPI)
```

---

## License

MIT
