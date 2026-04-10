# isi-music

[![Release](https://img.shields.io/github/v/release/glrmrissi/isi_music?style=flat-square&color=1DB954&label=version)](https://github.com/glrmrissi/isi_music/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/glrmrissi/isi_music/ci.yml?style=flat-square&label=build)](https://github.com/glrmrissi/isi_music/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/glrmrissi/isi_music?style=flat-square)](LICENSE)

A terminal music player written in Rust. Stream from Spotify via librespot or play local audio files — no browser, no Electron.

![preview](https://github.com/user-attachments/assets/f67383b5-cc7d-4486-8d7a-ffea9ad2e997)

## Features

- **Spotify streaming** via librespot — no Spotify app required
- **Local file playback** — MP3, FLAC, OGG, WAV, AIFF, no account needed
- **Real-time audio visualizer** using braille characters (works for both Spotify and local files)
- Full-text search across tracks, albums, artists, playlists, and podcasts
- Queue management with cross-player support (mix Spotify and local tracks)
- Shuffle and repeat modes (off / queue / track)
- Album art rendered via Kitty / Sixel / half-block (terminal auto-detected)
- **MPRIS2 D-Bus** — media keys, Waybar widget, `playerctl` support
- **Last.fm scrobbling**
- **Daemon mode** — keep playback running after closing the terminal, control via CLI

> **Spotify Premium is required for streaming.** Local file playback works without any Spotify account.

---

## Installation

### Linux

**1. Download the binary**

```bash
curl -L https://github.com/glrmrissi/isi_music/releases/latest/download/isi-music-linux -o isi-music
chmod +x isi-music
sudo mv isi-music /usr/local/bin/
```

**2. Install audio dependencies**

| Distro | Command |
|--------|---------|
| Debian / Ubuntu | `sudo apt install libasound2 libpulse0` |
| Arch Linux | `sudo pacman -S alsa-lib libpulse` |
| Fedora | `sudo dnf install alsa-lib pulseaudio-libs` |

**3. Run**

```bash
isi-music
```

---

### Windows

**1. Download `isi-music-windows.exe`** from the [Releases page](https://github.com/glrmrissi/isi_music/releases/latest).

**2.** No extra dependencies — audio uses WASAPI, which is built into Windows.

**3. Run** (Windows Terminal recommended for best rendering):

```powershell
.\isi-music-windows.exe
```

> For proper album art and UI rendering, use a terminal with true color support and a [Nerd Font](https://www.nerdfonts.com/).

---

### Build from Source

Requires Rust 1.85+ (edition 2024).

**Linux:**
```bash
# Install build dependencies (Debian/Ubuntu)
sudo apt install libasound2-dev libpulse-dev libdbus-1-dev

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

The config file is created automatically on first run:

| Platform | Path |
|----------|------|
| Linux | `~/.config/isi-music/config.toml` |
| Windows | `%APPDATA%\isi-music\config.toml` |

```toml
# Required only for Spotify streaming — omit if using local files only
[spotify]
client_id = "your_client_id_here"

# Optional: local audio files (MP3, FLAC, OGG, WAV, AIFF)
[local]
music_dir = "~/Music"

# Optional: Last.fm scrobbling
[lastfm]
api_key    = "your_lastfm_api_key"
api_secret = "your_lastfm_api_secret"
session_key = "obtained_via_setup-lastfm"
```

### Spotify Setup

1. Register an app at [developer.spotify.com](https://developer.spotify.com/dashboard)
2. Set the redirect URI to `http://127.0.0.1:8888/callback`
3. Copy the Client ID into `config.toml`

isi-music uses **PKCE authentication** — only the Client ID is required. No client secret.

On first run with Spotify configured, a browser window opens for OAuth authorization. The token is cached and reused automatically on subsequent runs.

---

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous panel |
| `hjkl` / `↑↓` | Navigate within a panel |
| `Enter` | Play selected / open album or artist |
| `Space` | Play / pause |
| `n` / `p` | Next / previous track |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off → queue → track) |
| `+` / `-` | Volume up / down |
| `←` / `→` | Seek ±5 s |
| `/` | Search |
| `a` | Add track to queue |
| `Delete` | Remove selected item from queue |
| `R` | Toggle Radio Mode (auto-recommendations when queue ends) |
| `Alt+r` | Get similar tracks for selected track or artist |
| `c` | Toggle album art panel |
| `z` | Toggle fullscreen player |
| `l` | Like current track |
| `Backspace` | Back to previous search results |
| `Esc` | Close search / exit fullscreen |
| `q` / `Ctrl+C` | Quit |

---

## Theme Configuration

```
# Place this file at: ~/.config/isi-music/theme.toml

# ================================================================
# THEME EXAMPLES
# ================================================================

# Minimal Dark Theme
# border_active = "cyan"
# border_inactive = "dark_gray"
# highlight_bg = "rgb(30, 30, 30)"
# text_primary = "white"
# accent_color = "cyan"

# Vibrant Theme
# border_active = "magenta"
# border_inactive = "dark_gray"
# highlight_bg = "rgb(203, 23, 203)"
# text_primary = "white"
# accent_color = "light_magenta"

# Cool Blue Theme
border_active = "light_blue"
border_inactive = "dark_gray"
highlight_bg = "rgb(20, 40, 60)"
text_primary = "white"
accent_color = "light_cyan"

# Warm Orange Theme
# border_active = "yellow"
# border_inactive = "dark_gray"
# highlight_bg = "rgb(60, 30, 20)"
# text_primary = "white"
# accent_color = "light_yellow"

# ================================================================
# AVAILABLE COLORS
# ================================================================
# Named colors (web-safe):
#   - black, dark_gray, gray, light_gray, white
#   - red, light_red
#   - green, light_green
#   - yellow, light_yellow
#   - blue, light_blue
#   - magenta, light_magenta
#   - cyan, light_cyan
#
# Custom RGB colors:
#   - rgb(r, g, b) where r, g, b are 0-255
#   - Examples: rgb(255, 0, 0), rgb(100, 200, 50), rgb(30, 30, 30)
#
# ================================================================
# COLOR MAPPING IN UI
# ================================================================
# border_active:      Focused panel borders, active status indicators
# border_inactive:    Unfocused panel borders, secondary text, timestamps
# highlight_bg:       Background of selected list items
# text_primary:       Track titles, artist names, primary UI text
# accent_color:       Progress bars, icons, loading indicators, seek bar
``

## Local Files

Play local audio files without any Spotify account. Supported formats: **MP3, FLAC, OGG, WAV, AIFF**.

### Setup

Add the `[local]` section to your config:

```toml
[local]
music_dir = "~/Music"
```

### Usage

1. Navigate to **Local Files** in the Library panel
2. Press `Enter` to scan the directory
3. Select a track and press `Enter` to play

Track metadata (title, artist, album) is read automatically from file tags. Files without tags fall back to the filename.

You can mix local and Spotify tracks in the same queue — isi-music routes each track to the appropriate player automatically.

> If no Spotify credentials are configured, isi-music starts in local-only mode and skips all Spotify features.

---

## Daemon Mode

Keep music playing after closing the terminal and control it from the command line.

```bash
# Start the daemon
isi-music --daemon

# Load a Spotify playlist and play
isi-music --play spotify:playlist:37i9dQZF1DXcBWIGoYBM5M

# Load all liked songs
isi-music --liked

# List loaded tracks
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

Logs are written to `~/.local/share/isi-music/isi-music.log`.

```bash
isi-music --clear-logs
```

---

## MPRIS2 / Linux Integration

isi-music registers on D-Bus as `org.mpris.MediaPlayer2.isi_music`, enabling integration with:

- **Media keys** (XF86AudioPlay / Next / Prev)
- **Waybar** `mpris` module
- **`playerctl`** CLI

**Waybar config:**
```json
"mpris": {
    "format": "{player_icon} {title} — {artist}",
    "player-icons": { "isi_music": "" },
    "status-icons": { "playing": "▶", "paused": "⏸", "stopped": "⏹" }
}
```

**Hyprland media key bindings:**
```
bind = , XF86AudioPlay, exec, playerctl play-pause
bind = , XF86AudioNext, exec, playerctl next
bind = , XF86AudioPrev, exec, playerctl previous
```

MPRIS works in both TUI and daemon mode.

---

## Last.fm Scrobbling

isi-music supports automatic scrobbling via the [Last.fm API](https://www.last.fm/api).

### Setup

Run the interactive wizard:

```bash
isi-music setup-lastfm
```

The wizard will:
1. Prompt for your **API Key** and **API Secret** — create an app at [last.fm/api/account/create](https://www.last.fm/api/account/create)
2. Open the Last.fm authorization page in your browser
3. Exchange the token for a session key automatically
4. Write everything to `~/.config/isi-music/config.toml`

### Behaviour

| Event | Action |
|-------|--------|
| Track starts | `track.updateNowPlaying` |
| Track reaches 50% playback | `track.scrobble` |

To disable scrobbling, remove the `[lastfm]` section from the config.

---

## How It Works

isi-music uses two audio backends depending on the source:

- **librespot** — handles Spotify authentication and streams audio via the Spotify Connect protocol
- **rodio + symphonia** — decodes and plays local audio files (MP3, FLAC, OGG, WAV, AIFF)

The Spotify Web API (**rspotify**) is used separately for search, metadata, and album art.

The TUI is built with [ratatui](https://github.com/ratatui-org/ratatui). A single event loop polls player events, processes keyboard input, and re-renders the UI at ~30 fps.

```
                         ┌─────────────────────┐
  Spotify ──────────────►│     librespot        │─┐
                         └─────────────────────┘  │
                                                   ├──► System audio
                         ┌─────────────────────┐  │    (ALSA / PulseAudio / WASAPI)
  Local files ──────────►│  rodio + symphonia   │─┘
                         └─────────────────────┘
                                   │
                         ┌─────────▼─────────┐
                         │   BandAnalyzer     │──► Real-time visualizer
                         │   (FFT, 64 bands)  │
                         └───────────────────┘

  Spotify Web API ──────► search, metadata, album art
```

---

## License

MIT
