# isi-music

[![Release](https://img.shields.io/github/v/release/glrmrissi/isi_music?style=flat-square&color=1DB954&label=version)](https://github.com/glrmrissi/isi_music/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/glrmrissi/isi_music/ci.yml?style=flat-square&label=build)](https://github.com/glrmrissi/isi_music/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/glrmrissi/isi_music?style=flat-square)](LICENSE)

isi-music is a terminal audio player for Spotify streaming and local file playback, built in Rust. It replaces resource-heavy desktop clients with a native TUI that runs in any terminal emulator.

## Features

- **Spotify streaming** via librespot -- no official Spotify app required
- **Local file playback** -- MP3, FLAC, Opus, Ogg Vorbis, WAV with automatic metadata extraction
- **Real-time audio visualizer** using braille characters (Spotify + local files)
- **Full-text search** across tracks, albums, artists, playlists, and podcasts
- **Queue management** with cross-player support (mix Spotify and local tracks)
- **Shuffle and repeat** modes (off / queue / track)
- **Album art** rendered via Kitty / Sixel / half-block (terminal auto-detected)
- **Embedded cover art** support for local files
- **MPRIS2 D-Bus** -- media keys, Waybar widget, playerctl support
- **Last.fm scrobbling** -- now playing + automatic scrobble at 50% or 4 minutes
- **Discord Rich Presence** -- shows current track in Discord activity
- **Daemon mode** -- keep playback after closing the terminal, control via CLI
- **Playlist management** -- add and remove tracks via keyboard (tiling picker)
- **Command mode** -- `:` prefix commands like `ap <search>`, `newplaylist <name>`
- **Session restoration** -- active view, compact mode, and volume are saved on quit; startup focus returns to Library
- **Large-library pagination** -- playlists load 50 items at a time as you approach the end; liked songs synchronize across all API pages
- **Bounded search and audio caches** -- cached results are evicted instead of growing without limit
- **Virtualized list rendering** -- smooth navigation in large libraries and playlists
- **Mouse support** -- scroll wheel navigation in lists and lyrics
- **Direct panel shortcuts** -- `1`/`2`/`3`/`4` jump to Library / Playlists / Tracks / Queue
- **Jump to playing track** -- `c` centers the current track in the list
- **Seek support** for all audio formats

> Spotify Premium is required for streaming. Local file playback works without any Spotify account.
> See the [Spotify Setup](#spotify-setup) section below.

## Quick Install

The install scripts download the latest binary, install a Nerd Font (if missing), install audio dependencies (Linux), and launch the setup wizard automatically.

**Linux:**
```bash
curl -fsSL https://raw.githubusercontent.com/glrmrissi/isi_music/main/scripts/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/glrmrissi/isi_music/main/scripts/install.ps1 | iex
```

After installation, run `isi-music` to start. If something isn't working, run `isi-music doctor` to diagnose common issues.

On Windows, the installer also creates an `isi-music` shortcut in the Start Menu when Windows Terminal is installed. The shortcut opens the app in Windows Terminal with PowerShell; running `isi-music.exe` directly still works from any console.

## Getting Started

### Prerequisites: Nerd Font

A Nerd Font is required for proper album art and UI rendering.

**Linux:**
```bash
mkdir -p ~/.local/share/fonts
unzip NerdFont.zip -d ~/.local/share/fonts
fc-cache -fv
```

**macOS:**
```bash
brew tap homebrew/cask-fonts
brew install font-fira-code-nerd-font
```

**Windows:**
Download from https://www.nerdfonts.com/font-downloads, extract, right-click and install the .ttf files.

Configure your terminal to use the font (e.g. "FiraCode Nerd Font" or "JetBrains Mono Nerd Font").

### Download

**Linux:**
```bash
curl -L https://github.com/glrmrissi/isi_music/releases/latest/download/isi-music-linux-x86_64 -o isi-music
chmod +x isi-music
sudo mv isi-music /usr/local/bin/
```

**macOS:**
```bash
curl -L https://github.com/glrmrissi/isi_music/releases/latest/download/isi-music-macos-arm64 -o isi-music
chmod +x isi-music
sudo mv isi-music /usr/local/bin/
```

**Windows:**
```powershell
# PowerShell
Invoke-WebRequest -Uri https://github.com/glrmrissi/isi_music/releases/latest/download/isi-music-windows-x86_64.exe -OutFile isi-music.exe
# Move it to a folder on your PATH, e.g.:
mkdir "$env:LOCALAPPDATA\Programs\isi-music" -Force
move isi-music.exe "$env:LOCALAPPDATA\Programs\isi-music\"
```

No audio dependencies are needed on Windows (WASAPI is built in). Windows Terminal is recommended for true-color rendering, mouse support, and the best TUI experience. PowerShell is the default shell used by the installer shortcut.

**Linux audio dependencies:**

| Distro | Command |
|--------|---------|
| Debian / Ubuntu | `sudo apt install libasound2t64 libpulse0` |
| Arch Linux | `sudo pacman -S alsa-lib libpulse` |
| Fedora | `sudo dnf install alsa-lib pulseaudio-libs` |

### First Run

```bash
isi-music
```

On first launch, run the setup wizard to configure Spotify, Last.fm, and theme:

```bash
isi-music setup
```

Individual setup commands:

```bash
isi-music setup-spotify   # Spotify Client ID + PKCE OAuth
isi-music setup-lastfm    # Last.fm authentication (no API key needed!)
isi-music doctor          # Diagnose common issues (Nerd Font, audio deps, config, etc.)
isi-music update          # Update to the latest release from GitHub
```

## Configuration

All config lives under `~/.config/isi-music/` on Linux, `~/Library/Application Support/isi-music/` on macOS, and `%APPDATA%\isi-music\` on Windows.

```toml
[spotify]
client_id = "your_client_id_here"

[local]
music_dir = "~/Music"

[lastfm]
session_key = "obtained_via_setup-lastfm"

[discord]
enabled = true

[musixmatch]
musixmatch_api_key = "your_musixmatch_api_key"
```

The `[session]` section is written automatically when you quit and stores the active view, compact mode, and volume. The saved focus value is retained for compatibility, but the app starts with focus on Library:

```toml
[session]
focus = "tracks"
active_content = "tracks"
compact_mode = false
library_selected = 0
volume = 80
```

See [Spotify Setup](#spotify-setup) for obtaining a Client ID.

### Library and cache behavior

Large Spotify collections are loaded incrementally where possible:

- **Liked Songs** are synchronized across all Spotify API pages and stored in the local SQLite library cache.
- **Playlists, albums, and shows** load additional 50-item pages when navigation reaches the end of the currently loaded list.
- The in-memory search cache keeps at most 32 recent queries and expires entries after 10 minutes.
- The Spotify audio cache is stored on disk, with a limit of 1 GiB. It is not a permanent in-memory allocation.

Relevant paths:

| Data | Linux | Windows |
|------|-------|---------|
| Library/search database | `~/.local/share/isi-music/library.db` | `%APPDATA%\\isi-music\\library.db` |
| Spotify audio cache | `~/.cache/isi-music/audio-cache/` | `%LOCALAPPDATA%\\isi-music\\audio-cache\\` |

### Theme

Create `~/.config/isi-music/theme.toml` to customize colors and layout.

```toml
border_active = "#00d4ff"
border_inactive = "#ffffff"
highlight_bg = "#004b7a"
text_primary = "#ffffff"
accent_color = "#ffeb3b"
background = "#141414"
text_secondary = "#888888"
status_bar = "#1e1e1e"
show_ascii_art = false
highlight_symbol = "> "
options_panel_symbol = "▶ "
```

**Color reference:**

| Variable | Purpose |
|----------|---------|
| `border_active` | Focused panel borders, active indicators |
| `border_inactive` | Unfocused borders |
| `border_subtle` | Subtle panel borders and dividers |
| `border_dimmest` | Lowest-contrast borders |
| `highlight_bg` | Selected list items background |
| `text_primary` | Titles, artists, primary text |
| `accent_color` | Progress bars, icons, seek bar |
| `background` | Root background fill |
| `background_panel` | Panel background layer |
| `background_element` | Element/background layer |
| `text_secondary` | Subtle text, timestamps, metadata |
| `status_bar` | Bottom status bar background |
| `primary` | Primary accent role |
| `success`, `warning`, `error`, `info` | Semantic status colors |
| `highlight_symbol` | List selection indicator (default: `"> "`) |
| `options_panel_symbol` | Options panel selection indicator (default: `"▶ "`) |

Colors can be specified as hex (`#rrggbb`), named (`white`, `red`, `green`, etc.), or RGB function (`rgb(r,g,b)`).

#### Visualizer and reactive theme

The visualizer can be configured per theme:

```toml
[visualizer]
style = "braille_bars" # braille_bars, plasma, or anime_art
color = "#82aaff"     # optional; defaults to accent_color
height = 8             # optional maximum height in terminal rows
art_path = "assets/anime_art.txt" # used by anime_art
```

To derive colors from the current album art and cross-fade between tracks:

```toml
reactive_theme = true
reactive_cross_fade_ms = 800
```

The same reactive-theme toggle is available in the options panel.

#### Per-widget borders

Border style and colors can be overridden for individual widgets. Supported styles are `rounded`, `thick`, `left_bar`, and `none`:

```toml
[borders.library]
style = "rounded"
color_focused = "#82aaff"
color_unfocused = "#737aa2"

[borders.visualizer]
style = "none"
```

The theme file supports custom layout trees, widget styles, and ASCII art:

```toml
border_active = "#00d4ff"
border_inactive = "#ffffff"
highlight_bg = "#004b7a"
text_primary = "#ffffff"
accent_color = "#ffeb3b"
background = "#141414"
text_secondary = "#888888"
status_bar = "#1e1e1e"

ascii_art_inline = [
    "      .---.         ",
    '     /|66_\        ',
    '     \| ^ /---.    ',
    "      |'-'| UI |   ",
    "      |   |____|   ",
    "      |   |        ",
    "      '---'        ",
    "    _________      ",
    "   /        /|     ",
    "  /________/ |     ",
    "  |        | |     ",
    "  |  ISI   | /     ",
    "  |________|/      ",
]
show_ascii_art = false

[widget_styles]

[layout_tree]
direction = "vertical"

[[layout_tree.constraints]]
length = 3

[[layout_tree.constraints]]
fill = 1

[[layout_tree.constraints]]
length = 1

[[layout_tree.constraints]]
length = 1

[[layout_tree.children]]
direction = "horizontal"

[[layout_tree.children.constraints]]
fill = 1

[[layout_tree.children.constraints]]
length = 40

[[layout_tree.children.children]]
widget = "header"

[[layout_tree.children.children]]
widget = "visualizer"

[[layout_tree.children]]
direction = "horizontal"

[[layout_tree.children.constraints]]
percentage = 20

[[layout_tree.children.constraints]]
fill = 1

[[layout_tree.children.children]]
direction = "vertical"

[[layout_tree.children.children.constraints]]
length = 7

[[layout_tree.children.children.constraints]]
length = 15

[[layout_tree.children.children.constraints]]
fill = 1

[[layout_tree.children.children.children]]
widget = "library"

[[layout_tree.children.children.children]]
widget = "playlists"

[[layout_tree.children.children.children]]
widget = "ascii_art"

[[layout_tree.children.children]]
direction = "vertical"

[[layout_tree.children.children.constraints]]
fill = 1

[[layout_tree.children.children.constraints]]
length = 8

[[layout_tree.children.children.children]]
widget = "main_content"

[[layout_tree.children.children.children]]
widget = "queue"

[[layout_tree.children]]
direction = "horizontal"

[[layout_tree.children.constraints]]
percentage = 30

[[layout_tree.children.constraints]]
fill = 1

[[layout_tree.children.children]]
widget = "marquee"

[[layout_tree.children.children]]
widget = "progress"

[[layout_tree.children]]
widget = "help"

[compact_layout]
direction = "vertical"

[[compact_layout.constraints]]
length = 1

[[compact_layout.constraints]]
fill = 1

[[compact_layout.constraints]]
length = 1

[[compact_layout.children]]
widget = "header"

[[compact_layout.children]]
direction = "horizontal"

[[compact_layout.children.constraints]]
percentage = 35

[[compact_layout.children.constraints]]
fill = 1

[[compact_layout.children.children]]
widget = "ascii_art"

[[compact_layout.children.children]]
widget = "main_content"

[[compact_layout.children]]
direction = "horizontal"

[[compact_layout.children.constraints]]
percentage = 30

[[compact_layout.children.constraints]]
fill = 1

[[compact_layout.children.children]]
widget = "marquee"

[[compact_layout.children.children]]
widget = "progress"

[fullscreen_layout]
direction = "vertical"

[[fullscreen_layout.constraints]]
length = 18

[[fullscreen_layout.constraints]]
length = 8

[[fullscreen_layout.constraints]]
min = 0

[[fullscreen_layout.children]]
widget = "now_playing"

[[fullscreen_layout.children]]
widget = "fullscreen_lyrics"

[[fullscreen_layout.children]]
widget = "visualizer"
```

The default palette is Neutral Dark: a restrained dark theme that leaves Tokyo Night and other color schemes as explicit presets. Run the wizard (`isi-music setup`) to choose from the available color presets while preserving your existing layout settings.

### Custom Keybindings

Create `~/.config/isi-music/keybinds.toml` to override default keybindings. See the full action list in the [Keybindings](#keybindings) section.

```toml
[navigation]
focus_library = ["1"]
focus_playlists = ["2"]
focus_tracks = ["3"]
focus_queue = ["4"]
nav_middle = ["M"]
jump_to_playing = ["c"]

[modes]
quick_search = ["ctrl+f"]
toggle_compact = ["m"]
toggle_fullscreen = ["z"]

[actions]
command_prompt = [":"]
```

Each key is a string of the form `modifier+key` (e.g. `ctrl+f`, `alt+d`, `shift+b`). Uppercase letters infer `shift`. Multiple keys can map to the same action (`["up", "k"]`).

## Spotify Setup

The February 2026 Spotify Web API changes require a Client ID for all API requests.

1. Go to the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard) and click **"Create app"**
2. Set the Redirect URI to **`http://127.0.0.1:8888/callback`**
3. Copy the Client ID
4. Run `isi-music setup-spotify` or add it to `config.toml` manually:

```toml
[spotify]
client_id = "your_client_id_here"
```

The setup wizard uses PKCE OAuth -- no `client_secret` is required. Your browser will open for authorization.

## Usage

### Keybindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous panel |
| `↑` / `↓` or `k` / `j` | Navigate within a panel |
| `Ctrl+↑` / `Ctrl+↓` | First / last item |
| `M` | Jump to the middle of the current list |
| `1` / `2` / `3` / `4` | Focus Library / Playlists / Tracks / Queue |
| `c` | Jump to the currently playing track in the list |
| `Enter` | Play selected / open album or artist |
| `Space` | Play / pause |
| `n` / `p` | Next / previous track |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off -> queue -> track) |
| `+` / `-` | Volume up / down |
| `←` / `→` | Seek +/- 5s (hold for +/- 10s) |
| `5` | Seek to 50% of the current track |
| `/` | Search |
| `Esc` | Back / close search / exit fullscreen |
| `q` or `Ctrl+C` | Quit |

**Playlist & Library:**

| Key | Action |
|-----|--------|
| `l` | Like current track |
| `A` | Add selected track to playlist (tiling picker) |
| `D` | Remove selected track from playlist / unlike |
| `:` | Command prompt (`ap <search>`, `newplaylist <name>`) |
| `a` | Add track to queue |
| `Delete` | Remove selected item from queue |
| `o` | Sort tracks (default / title / artist / album / duration) |
| `R` | Toggle radio mode (auto-recommendations) |
| `Alt+r` | Get similar tracks for selection |
| `Ctrl+y` | Copy track link to clipboard |

**Display:**

| Key | Action |
|-----|--------|
| `z` | Toggle fullscreen player |
| `m` | Toggle compact mode |
| `v` | Toggle visualizer |
| `y` | Toggle lyrics |
| `d` | Toggle debug overlay |
| `Shift+b` | Toggle breadcrumb |
| `t` | Open options panel |
| `?` | Open help panel |
| `Ctrl+f` | Quick search (filter current track list) |
| `PgUp` / `PgDown` | Scroll lists / lyrics |
| Mouse wheel | Scroll the focused list or lyrics |
All keybindings are customizable in `keybinds.toml`.

### Daemon Mode

Keep playback running in the background, controlled from the command line.

```bash
# Start the daemon
isi-music --daemon

# Load and play
isi-music --play spotify:playlist:37i9dQZF1DXcBWIGoYBM5M
isi-music --liked

# List loaded tracks
isi-music --ls

# Play by index
isi-music --play-id 2

# Playback controls
isi-music --toggle       # play / pause
isi-music --next         # next track
isi-music --prev         # previous track
isi-music --vol+         # volume +5%
isi-music --vol-         # volume -5%

# Query status
isi-music --status       # shows current track and progress

# Stop the daemon
isi-music --quit-daemon
```

Logs are written to `~/.cache/isi-music/isi-music.log` (Linux) or the equivalent cache path on other platforms (`%LOCALAPPDATA%\isi-music\` on Windows). Clear them with `isi-music --clear-logs`.

> Daemon mode currently supports Spotify playback only. Local file playback works in TUI mode.
>
> **Windows note:** instead of forking, `--daemon` launches a detached background process, and
> CLI <-> daemon IPC uses the named pipe `\\.\pipe\isi-music` instead of a Unix socket. All CLI
> commands work the same. Media keys are not supported (MPRIS is Linux-only).

## Local Files

isi-music can play local audio files without a Spotify account. Point it at your music directory in `config.toml`:

```toml
[local]
music_dir = "~/Music"
```

> **Windows:** use forward slashes (`music_dir = "C:/Users/you/Music"`) or double the
> backslashes (`"C:\\Users\\you\\Music"`). A single `\U` sequence is an invalid TOML
> escape and will prevent the app from starting.

Supported formats: MP3, FLAC, Opus (.opus), Ogg Vorbis, WAV.

Navigate to **Local Files** in the library panel and press Enter to scan. The first scan extracts metadata and embedded cover art, cached in SQLite for instant subsequent loads.

You can mix local and Spotify tracks in the same queue -- isi-music routes each track to the appropriate player automatically.

## Integrations

### MPRIS2 (Linux only)

isi-music registers on D-Bus as `org.mpris.MediaPlayer2.isi_music`, enabling media keys, Waybar widgets, and `playerctl`. Not available on Windows or macOS (no D-Bus session bus).

**Waybar config:**
```json
"mpris": {
    "format": "{player_icon} {title} -- {artist}",
    "player-icons": { "isi_music": "" },
    "status-icons": { "playing": ">", "paused": "||" }
}
```

**Hyprland media key bindings:**
```
bind = , XF86AudioPlay, exec, playerctl play-pause
bind = , XF86AudioNext, exec, playerctl next
bind = , XF86AudioPrev, exec, playerctl previous
```

MPRIS works in both TUI and daemon modes.

### Last.fm Scrobbling

Run `isi-music setup-lastfm` to configure. The wizard will open your browser for Last.fm authorization - just log in and authorize the app. No API credentials needed!

Scrobbling behavior:
- Track starts: `track.updateNowPlaying`
- Track reaches 50% or 4 minutes (whichever comes first): `track.scrobble`

To disable, remove the `[lastfm]` section from your config.

### Discord Rich Presence

Enable in `config.toml`:
```toml
[discord]
enabled = true
```

Optional: use a custom app ID (default: isi-music official app):
```toml
[discord]
enabled = true
app_id = "your_custom_app_id"
```

Your status will show "Listening to [Track] by [Artist]".

## Development

### Build from Source

Requires Rust 1.85+ (edition 2024).

```bash
git clone https://github.com/glrmrissi/isi_music.git
cd isi_music

# Linux build dependencies
sudo apt install libasound2-dev libpulse-dev libdbus-1-dev pkg-config

# Windows: MSVC C++ Build Tools + cmake (Opus builds libopus from C sources)
winget install Kitware.CMake

# Linux: cmake is also required (Opus builds libopus from C sources)
sudo apt install cmake

cargo build --release

# Run with debug logging
RUST_LOG=isi_music=debug cargo run   # PowerShell: $env:RUST_LOG="isi_music=debug"; cargo run

# Run tests
cargo test
```

### Build Variants

Pre-built binaries come in two variants:

| Variant | Size | Features | Use Case |
|---------|------|----------|----------|
| `isi-music-<platform>` | ~10 MB | All features (album art, visualizer, wizard, lyrics, MPRIS, Discord) | Full experience (MPRIS included via `-F mpris` in CI) |
| `isi-music-<platform>-minimal` | ~9 MB | Spotify streaming, Discord, Last.fm, setup (no album art, visualizer, lyrics, MPRIS) | Headless daemon or minimal TUI |

### Feature Flags

Build with specific features using the `-F` flag:

```bash
# Minimal build (streaming + Discord only)
cargo build --release --no-default-features -F spotify,discord

# Add MPRIS back if needed
cargo build --release --no-default-features -F spotify,discord,mpris

# Exclude album art (smaller binary, fewer deps)
cargo build --release --no-default-features -F spotify,discord,mpris,lastfm,wizard,visualizer,lyrics
```

Available features:

| Feature | Default | Description |
|---------|---------|-------------|
| `spotify` | yes | Spotify streaming via librespot |
| `discord` | yes | Discord Rich Presence |
| `mpris` | no | MPRIS2 D-Bus media controls (Linux only; no-op on other platforms) |
| `lastfm` | yes | Last.fm scrobbling |
| `wizard` | yes | Interactive setup wizard |
| `visualizer` | yes | Real-time audio FFT visualizer |
| `lyrics` | yes | Synced and unsynced lyrics fetching |
| `album-art` | yes | Album art rendering (Kitty/Sixel/half-block) |

### How It Works

isi-music uses multiple audio backends depending on the source:

- **librespot** -- Spotify authentication and audio streaming via the Spotify Connect protocol
- **rodio + symphonia** -- Local audio decoding (MP3, FLAC, Ogg Vorbis, WAV)
- **ogg + opus2 + opusmeta** -- Opus decoding (RFC 7845) with libopus (bundled)
- **Custom HTTP client** -- Spotify Web API for search, metadata, playlists, and album art

The TUI is built with ratatui. The event loop polls player state, processes keyboard input, and renders at ~60 fps.

Tests live in `tests/` mirroring the `src/` structure, referenced via `#[path]` attributes.

### Versioning

This project follows Semantic Versioning derived from conventional commits:

| Commit type | Version bump |
|-------------|--------------|
| `fix:` | Patch (1.0.x) |
| `feat:` | Minor (1.x.0) |
| `BREAKING CHANGE` footer | Major (x.0.0) |

## Troubleshooting

### Local files showing "Unknown Artist"

Delete the SQLite cache and covers, then restart:

**Linux:**
```bash
rm ~/.local/share/isi-music/library.db
rm -rf ~/.cache/isi-music/covers/
```

**Windows (PowerShell):**
```powershell
Remove-Item "$env:APPDATA\isi-music\library.db"
Remove-Item -Recurse "$env:LOCALAPPDATA\isi-music\covers\"
```

### Album art not showing

- Ensure your terminal supports true color: `echo $COLORTERM`
- Verify a Nerd Font is installed and configured
- Check that embedded artwork exists in your audio files

### MPRIS not working (Linux)

- Ensure D-Bus is running: `systemctl --user status dbus`
- Check that `DBUS_SESSION_BUS_ADDRESS` is set: `echo $DBUS_SESSION_BUS_ADDRESS`

## License

MIT
