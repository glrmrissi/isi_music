# isi-music

### Please, if you are not a developer, download the version that is in the release :)

[![Release](https://img.shields.io/github/v/release/glrmrissi/isi_music?style=flat-square&color=1DB954&label=version)](https://github.com/glrmrissi/isi_music/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/glrmrissi/isi_music/ci.yml?style=flat-square&label=build)](https://github.com/glrmrissi/isi_music/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/glrmrissi/isi_music?style=flat-square)](LICENSE)

A terminal music player written in Rust. Stream from Spotify via librespot or play local audio files with full metadata support — no browser, no Electron, no bloat.

![preview](https://github.com/user-attachments/assets/f67383b5-cc7d-4486-8d7a-ffea9ad2e997)
![purple](https://github.com/user-attachments/assets/27d7420e-918f-4d04-9986-34301b60d22c)
![blue](https://github.com/user-attachments/assets/2f5cd6f4-d403-4c98-aa80-fc8d5df847d5)

## Features

- **Spotify streaming** via librespot — no Spotify app required
- **Local file playback** — MP3, FLAC, OGG, WAV, AIFF with automatic metadata extraction
- **Real-time audio visualizer** using braille characters (works for both Spotify and local files)
- Full-text search across tracks, albums, artists, playlists, and podcasts
- Queue management with cross-player support (mix Spotify and local tracks)
- Shuffle and repeat modes (off / queue / track)
- Album art rendered via Kitty / Sixel / half-block (terminal auto-detected)
- Embedded cover art support for local files
- **MPRIS2 D-Bus** — media keys, Waybar widget, `playerctl` support
- **Last.fm scrobbling**
- **Daemon mode** — keep playback running after closing the terminal, control via CLI
- **Seek support** for all audio formats

Note: Spotify Premium is required for streaming. Local file playback works without any Spotify account.

---

## Installation

### Prerequisites: Nerd Font

For proper album art and UI rendering, install a Nerd Font on your system:

#### Linux

1. Download a Nerd Font from https://www.nerdfonts.com/font-downloads
2. Extract to a fonts directory:
   ```bash
   mkdir -p ~/.local/share/fonts
   unzip NerdFont.zip -d ~/.local/share/fonts
   fc-cache -fv
   ```
3. Configure your terminal to use the Nerd Font (e.g., "FiraCode Nerd Font" or "JetBrains Mono Nerd Font")

#### Windows

1. Download a Nerd Font from https://www.nerdfonts.com/font-downloads
2. Extract the .zip file
3. Right-click any .ttf file and select "Install for all users"
4. Configure Windows Terminal or your terminal emulator to use the installed Nerd Font:
   - Windows Terminal: Settings > Appearance > Font face
   - VS Code: User Settings > Terminal > Integrated: Font Family

#### macOS

1. Install via Homebrew (recommended):
   ```bash
   brew tap homebrew/cask-fonts
   brew install font-fira-code-nerd-font
   ```
   Or download manually from https://www.nerdfonts.com/font-downloads

2. Extract to `~/Library/Fonts`
3. Configure your terminal (iTerm2, Terminal.app, or Alacritty) to use the Nerd Font

Popular choices: FiraCode Nerd Font, JetBrains Mono Nerd Font, Meslo Nerd Font

---

### Linux

**1. Download the binary**

```bash
curl -L https://github.com/glrmrissi/isi_music/releases/latest/download/isi-music-linux-x86_64 -o isi-music
chmod +x isi-music
sudo mv isi-music /usr/local/bin/
```

**2. Install audio dependencies**

| Distro | Command |
|--------|---------|
| Debian / Ubuntu | `sudo apt install libasound2t64 libpulse0` |
| Arch Linux | `sudo pacman -S alsa-lib libpulse` |
| Fedora | `sudo dnf install alsa-lib pulseaudio-libs` |
| Alpine Linux | `apk add alsa-lib pulseaudio-libs` |

**3. Run**

```bash
isi-music
```

---

### Windows

**1. Download `isi-music-windows.exe`** from the [Releases page](https://github.com/glrmrissi/isi_music/releases/latest).

**2.** No extra dependencies — audio uses WASAPI, which is built into Windows.

**3. Run** (Windows Terminal recommended):

```powershell
.\isi-music-windows.exe
```

For best experience, ensure Windows Terminal has a Nerd Font configured.

---

### macOS

**1. Download the binary**

```bash
curl -L https://github.com/glrmrissi/isi_music/releases/latest/download/isi-music-macos-arm64  -o isi-music
chmod +x isi-music
sudo mv isi-music /usr/local/bin/
```

**2. Grant audio permissions** (macOS may prompt on first run)

**3. Run**

```bash
isi-music
```

---

### Build from Source

Requires Rust 1.85+ (edition 2024).

**Linux:**
```bash
# Install build dependencies (Debian/Ubuntu)
sudo apt install libasound2-dev libpulse-dev libdbus-1-dev pkg-config

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

**macOS:**
```bash
git clone https://github.com/glrmrissi/isi_music.git
cd isi_music
cargo build --release
./target/release/isi-music
```

---

## Configuration

The config file is created automatically on first run:

| Platform | Path |
|----------|------|
| Linux | `~/.config/isi-music/config.toml` |
| macOS | `~/Library/Application Support/isi-music/config.toml` |
| Windows | `%APPDATA%\isi-music\config.toml` |

```toml
# Required only for Spotify streaming — omit if using local files only
[spotify]
client_id = "your_client_id_here"

# Optional: local audio files (MP3, FLAC, OGG, WAV, AIFF)
# Automatic metadata extraction and embedded cover art support
[local]
music_dir = "~/Music" # If this path don't work, use entire path

# Optional: Last.fm scrobbling
[lastfm]
api_key    = "your_lastfm_api_key"
api_secret = "your_lastfm_api_secret"
session_key = "obtained_via_setup-lastfm"

# Optional: Discord Rich Presence
[discord]
enabled = true
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
| `←` / `→` | Seek ±5 s (hold for ±10 s) |
| `/` | Search |
| `a` | Add track to queue |
| `Delete` | Remove selected item from queue |
| `R` | Toggle Radio Mode (auto-recommendations) |
| `Alt+r` | Get similar tracks |
| `c` | Toggle album art panel |
| `z` | Toggle fullscreen player |
| `l` | Like current track |
| `Backspace` | Back to previous search results |
| `Esc` | Close search / exit fullscreen |
| `q` / `Ctrl+C` | Quit |

---

## Theme Configuration

Themes are fully customizable. Create `~/.config/isi-music/theme.toml` to override the default theme.

```toml
# Cool Blue Theme
border_active = "light_blue"
border_inactive = "dark_gray"
highlight_bg = "rgb(20, 40, 60)"
text_primary = "white"
accent_color = "light_cyan"
```

### Theme Examples

```toml
# Minimal Dark
border_active = "cyan"
border_inactive = "dark_gray"
highlight_bg = "rgb(30, 30, 30)"
text_primary = "white"
accent_color = "cyan"

# Vibrant
border_active = "magenta"
border_inactive = "dark_gray"
highlight_bg = "rgb(203, 23, 203)"
text_primary = "white"
accent_color = "light_magenta"

# Blood Moon
border_active = "red"
border_inactive = "rgb(146, 7, 7)"
highlight_bg = "rgb(50, 10, 10)"
text_primary = "white"
accent_color = "light_red"

# macOS Blue
border_active = "rgb(0,122,255)"
border_inactive = "rgb(60,60,60)"
highlight_bg = "rgb(45,45,45)"
text_primary = "white"
accent_color = "rgb(0,122,255)"
```

### Available Colors

Named colors: black, dark_gray, gray, light_gray, white, red, light_red, green, light_green, yellow, light_yellow, blue, light_blue, magenta, light_magenta, cyan, light_cyan

Custom RGB: `rgb(r, g, b)` where r, g, b are 0-255

### Color Mapping

- `border_active`: Focused panel borders, active indicators
- `border_inactive`: Unfocused borders, secondary text
- `highlight_bg`: Selected list items background
- `text_primary`: Titles, artists, primary text
- `accent_color`: Progress bars, icons, seek bar

### Layout Customization

See `theme.toml` documentation for advanced layout tree configuration.

---

## Local Files

Play local audio files without any Spotify account. Supported formats: MP3, FLAC, OGG, WAV, AIFF, Opus.

### Setup

Add the `[local]` section to your config:

```toml
[local]
music_dir = "~/Music"
```

### Features

- Automatic metadata extraction from ID3v2 (MP3), Vorbis Comments (FLAC/OGG), and other standard tags
- Embedded album art extraction and caching
- Full seek support for all formats
- Cross-format queue mixing (Spotify + local files)

### Usage

1. Navigate to "Local Files" in the Library panel
2. Press `Enter` to scan the directory (first run may take time depending on library size)
3. Select a track and press `Enter` to play

Files without complete metadata fall back to the filename. Metadata is cached in SQLite for fast subsequent loads.

You can mix local and Spotify tracks in the same queue — isi-music routes each track to the appropriate player automatically.

Note: If no Spotify credentials are configured, isi-music starts in local-only mode and skips all Spotify features.

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
# >  1  Creep — Radiohead
#    2  Fake Plastic Trees — Radiohead

# Play a specific track by ID
isi-music --play-id 2

# Playback controls
isi-music --toggle       # play / pause
isi-music --next         # next track
isi-music --prev         # previous track
isi-music --vol+         # volume +5%
isi-music --vol-         # volume -5%
isi-music --status       # >  Creep — Radiohead  |  1:23 / 3:58  |  vol 70%

# Stop the daemon
isi-music --quit-daemon
```

Logs are written to:
- Linux: `~/.local/share/isi-music/isi-music.log`
- macOS: `~/Library/Application Support/isi-music/isi-music.log`
- Windows: `%APPDATA%\isi-music\isi-music.log`

```bash
isi-music --clear-logs
```

---

## MPRIS2 / Linux Integration

isi-music registers on D-Bus as `org.mpris.MediaPlayer2.isi_music`, enabling integration with:

- Media keys (XF86AudioPlay / Next / Prev)
- Waybar `mpris` module
- `playerctl` CLI

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

MPRIS works in both TUI and daemon modes.

---

## Last.fm Scrobbling

isi-music supports automatic scrobbling via the Last.fm API.

### Setup

Run the interactive wizard:

```bash
isi-music setup-lastfm
```

The wizard will:
1. Prompt for your API Key and API Secret — create an app at [last.fm/api/account/create](https://www.last.fm/api/account/create)
2. Open the Last.fm authorization page in your browser
3. Exchange the token for a session key automatically
4. Write everything to your config file

### Behavior

- Track starts: `track.updateNowPlaying`
- Track reaches 50% playback: `track.scrobble`

To disable scrobbling, remove the `[lastfm]` section from the config.

---

## Discord Rich Presence

Show the current track in your Discord activity status.

### Setup

1. Enable Discord integration in your config:
   ```toml
   [discord]
   enabled = true
   ```

2. Optional: use a custom app ID (default: Anthropic's official app):
   ```toml
   [discord]
   enabled = true
   app_id = "your_custom_app_id"
   ```

3. Restart isi-music

Your Discord status will update to show: "Listening to [Track] by [Artist]"

---

## How It Works

isi-music uses multiple audio backends depending on the source:

- **librespot** — Spotify authentication and audio streaming via Spotify Connect protocol
- **rodio + symphonia** — Local audio decoding (MP3, FLAC, OGG, WAV, AIFF, Opus)

The Spotify Web API (rspotify) provides search, metadata, and album art.

The TUI is built with ratatui. The event loop polls player events, processes keyboard input, and renders at ~30 fps.

**Metadata Pipeline (Local Files):**

```
Audio file
    ↓
read_audio_metadata() ──► symphonia + ID3v2/metaflac
    ├─ title
    ├─ artist
    ├─ album
    ├─ duration
    └─ embedded cover art
    ↓
SQLite cache
    ├─ metadata (fast subsequent loads)
    └─ cover path (stored in ~/.cache/isi-music/covers/)
    ↓
LocalPlayer.current_track_info()
    ↓
PlaybackState
    ├─ title, artist, album, duration
    ├─ cover_path (for rendering)
    └─ path (file location)
    ↓
UI Render
    ├─ metadata display
    └─ embedded cover art
```

---

## Troubleshooting

### Local files showing "Unknown Artist"

1. Delete the SQLite cache and covers:
   ```bash
   rm ~/.local/share/isi-music/library.db
   rm -rf ~/.cache/isi-music/covers/
   ```

2. Restart isi-music and let it re-scan your library

3. Ensure your audio files have proper ID3v2 tags (MP3) or Vorbis comments (FLAC/OGG)

### Slow local file scanning

Large libraries (1000+ files) may take time on first scan. This is normal as metadata is being extracted and cached. Subsequent loads are instant.

### Album art not showing

- Ensure your terminal supports true color (check `echo $COLORTERM`)
- Verify a Nerd Font is installed and configured in your terminal
- Check that embedded artwork is present in your audio files

### MPRIS not working (Linux)

- Ensure D-Bus is running: `systemctl --user status dbus`
- Check that `DBUS_SESSION_BUS_ADDRESS` is set: `echo $DBUS_SESSION_BUS_ADDRESS`

---

## License

MIT
