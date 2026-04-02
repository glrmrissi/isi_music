# isi_music

[![Release](https://img.shields.io/github/v/release/glrmrissi/isi_music?style=flat-square&color=1DB954&label=version)](https://github.com/glrmrissi/isi_music/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/glrmrissi/isi_music/ci.yml?style=flat-square&label=build)](https://github.com/glrmrissi/isi_music/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/glrmrissi/isi_music?style=flat-square)](LICENSE)

A terminal-based Spotify player written in Rust. Uses librespot for native audio playback — no browser, no Electron, just your terminal.

<img width="1915" height="1031" alt="image" src="https://github.com/user-attachments/assets/ff4de1b1-488f-4d9c-8e8f-411c7990c0c7" />

## Features

- Native audio playback via librespot (no Spotify app required)
- Full-text search across tracks, albums, artists, playlists, and podcasts
- Queue management
- Shuffle and repeat modes (off / queue / track)
- Album art rendered via Kitty / Sixel / half-block (auto-detected)
- Audio visualizer using braille dots (2× horizontal + 4× vertical resolution)
- **MPRIS2 D-Bus integration** — media keys, Waybar widget, `playerctl` support
- Last.fm scrobbling
- Keyboard-driven TUI interface
- **Daemon mode** — keep music playing after closing the terminal, control via CLI

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

Register a Spotify app at [developer.spotify.com](https://developer.spotify.com/dashboard) and set the redirect URI to `http://127.0.0.1:8888/callback`.

isi-music uses **PKCE authentication** — only the Client ID is needed. No client secret required.

```toml
[spotify]
client_id = "your_client_id_here"

# Optional: Last.fm scrobbling
[lastfm]
api_key    = "your_lastfm_api_key"
api_secret = "your_lastfm_api_secret"
session_key = "obtained_via_setup-lastfm"
```

---

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` / `hjkl` / `↑↓` | Navigate panels |
| `Enter` | Play selected / open album or artist |
| `Space` | Play / pause |
| `n` / `p` | Next / previous track |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off → queue → track) |
| `+` / `-` | Volume up / down |
| `←` / `→` | Seek ±5 s (hold for ±10 s) |
| `/` | Search Spotify |
| `a` | Add track to queue |
| `c` | Toggle album art panel |
| `z` | Toggle fullscreen player |
| `l` | Like current track |
| `Backspace` | Back to previous search results |
| `Esc` | Close search / exit fullscreen |
| `q` / `Ctrl+C` | Quit |

---

## Daemon Mode

Run isi-music in the background — music keeps playing after the terminal is closed.

```bash
# Start the daemon (detaches from terminal automatically)
isi-music --daemon

# Load a playlist and play
isi-music --play spotify:playlist:37i9dQZF1DXcBWIGoYBM5M

# Load all liked songs and play
isi-music --liked

# List loaded tracks with their ID
isi-music --ls
#    0  Karma Police — Radiohead
# ▶  1  Creep — Radiohead
#    2  Fake Plastic Trees — Radiohead

# Play a specific track by ID
isi-music --play-id 2

# Playback controls
isi-music --toggle       # play / pause
isi-music --next         # next track
isi-music --prev         # previous track
isi-music --vol+         # volume +5%
isi-music --vol-         # volume -5%
isi-music --status       # ▶  Creep — Radiohead  |  1:23 / 3:58  |  vol 70%

# Stop the daemon
isi-music --quit-daemon
```

Daemon logs are written to `~/.local/share/isi-music/isi-music.log`.

```bash
# Clear the log file
isi-music --clear-logs
```

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

## MPRIS2 / Linux Integration

isi-music registers on D-Bus as `org.mpris.MediaPlayer2.isi_music`, so it works with:

- **Media keys** (XF86AudioPlay / Next / Prev) via Hyprland keybindings or `playerctld`
- **Waybar** `mpris` module — shows current track and controls playback
- **`playerctl`** CLI — `playerctl play-pause`, `playerctl next`, etc.

**Waybar config example:**
```json
"mpris": {
    "format": "{player_icon} {title} — {artist}",
    "player-icons": { "isi_music": "" },
    "status-icons": { "playing": "▶", "paused": "⏸", "stopped": "⏹" }
}
```

MPRIS works in both TUI mode and daemon mode. To use media keys in Hyprland:
```
bind = , XF86AudioPlay,  exec, playerctl play-pause
bind = , XF86AudioNext,  exec, playerctl next
bind = , XF86AudioPrev,  exec, playerctl previous
```

---

## License

MIT
