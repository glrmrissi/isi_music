# isi-music

[![Release](https://img.shields.io/github/v/release/glrmrissi/isi_music?style=flat-square&color=1DB954&label=version)](https://github.com/glrmrissi/isi_music/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/glrmrissi/isi_music/ci.yml?style=flat-square&label=build)](https://github.com/glrmrissi/isi_music/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/glrmrissi/isi_music?style=flat-square)](LICENSE)

isi-music is a terminal audio player for Spotify streaming and local file playback, built in Rust. It replaces resource-heavy desktop clients with a native TUI that runs in any terminal emulator.

## Features

- **Spotify streaming** via librespot -- no official Spotify app required
- **Local file playback** -- MP3 and FLAC with automatic metadata extraction
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
- **Seek support** for all audio formats

> Spotify Premium is required for streaming. Local file playback works without any Spotify account.
> See the [Spotify Setup](#spotify-setup) section below.

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
isi-music setup-lastfm    # Last.fm API key + session
```

## Configuration

All config lives under `~/.config/isi-music/` on Linux, `~/Library/Application Support/isi-music/` on macOS, and `%APPDATA%\isi-music\` on Windows.

```toml
[spotify]
client_id = "your_client_id_here"

[local]
music_dir = "~/Music"

[lastfm]
api_key    = "your_lastfm_api_key"
api_secret = "your_lastfm_api_secret"
session_key = "obtained_via_setup-lastfm"

[discord]
enabled = true

[musixmatch]
musixmatch_api_key = "your_musixmatch_api_key"
```

See [Spotify Setup](#spotify-setup) for obtaining a Client ID.

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
```

**Color reference:**

| Variable | Purpose |
|----------|---------|
| `border_active` | Focused panel borders, active indicators |
| `border_inactive` | Unfocused borders, secondary text |
| `highlight_bg` | Selected list items background |
| `text_primary` | Titles, artists, primary text |
| `accent_color` | Progress bars, icons, seek bar |
| `background` | Root background fill |
| `text_secondary` | Subtle text, timestamps, metadata |
| `status_bar` | Bottom status bar background |

Colors can be specified as hex (`#rrggbb`), named (`white`, `red`, `green`, etc.), or RGB function (`rgb(r,g,b)`).

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

Run the wizard (`isi-music setup`) to choose from 8 color presets that preserve your existing layout settings.

### Custom Keybindings

Create `~/.config/isi-music/keybinds.toml` to override default keybindings. See the full action list in the [Keybindings](#keybindings) section.

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
| `Enter` | Play selected / open album or artist |
| `Space` | Play / pause |
| `n` / `p` | Next / previous track |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off -> queue -> track) |
| `+` / `-` | Volume up / down |
| `←` / `→` | Seek +/- 5s (hold for +/- 10s) |
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
| `PgUp` / `PgDown` | Scroll lyrics |
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

Logs are written to `~/.cache/isi-music/isi-music.log` (Linux) or the equivalent cache path on other platforms. Clear them with `isi-music --clear-logs`.

> Daemon mode currently supports Spotify playback only. Local file playback works in TUI mode.

## Local Files

isi-music can play local audio files without a Spotify account. Point it at your music directory in `config.toml`:

```toml
[local]
music_dir = "~/Music"
```

Supported formats: MP3, FLAC.

Navigate to **Local Files** in the library panel and press Enter to scan. The first scan extracts metadata and embedded cover art, cached in SQLite for instant subsequent loads.

You can mix local and Spotify tracks in the same queue -- isi-music routes each track to the appropriate player automatically.

## Integrations

### MPRIS2 (Linux)

isi-music registers on D-Bus as `org.mpris.MediaPlayer2.isi_music`, enabling media keys, Waybar widgets, and `playerctl`.

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

Run `isi-music setup-lastfm` to configure. The wizard will prompt for your API credentials and open your browser for authorization.

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

cargo build --release

# Run with debug logging
RUST_LOG=isi_music=debug cargo run

# Run tests (154 tests)
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
| `mpris` | no | MPRIS2 D-Bus media controls |
| `lastfm` | yes | Last.fm scrobbling |
| `wizard` | yes | Interactive setup wizard |
| `visualizer` | yes | Real-time audio FFT visualizer |
| `lyrics` | yes | Synced and unsynced lyrics fetching |
| `album-art` | yes | Album art rendering (Kitty/Sixel/half-block) |

### How It Works

isi-music uses multiple audio backends depending on the source:

- **librespot** -- Spotify authentication and audio streaming via the Spotify Connect protocol
- **rodio + symphonia** -- Local audio decoding (MP3 and FLAC)
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
```bash
rm ~/.local/share/isi-music/library.db
rm -rf ~/.cache/isi-music/covers/
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
