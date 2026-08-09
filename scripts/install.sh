#!/usr/bin/env bash
#
# isi-music — Linux install script
#
# Downloads the latest binary from GitHub Releases, installs audio
# dependencies, installs a Nerd Font (if missing), and launches the
# setup wizard.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/glrmrissi/isi_music/main/scripts/install.sh | bash
#
set -euo pipefail

REPO="glrmrissi/isi_music"
BINARY_NAME="isi-music"
INSTALL_DIR="/usr/local/bin"
FONT_DIR="${HOME}/.local/share/fonts"
NERD_FONT_VERSION="3.3.0"
NERD_FONT_NAME="FiraCode"
NERD_FONT_ZIP="FiraCode.zip"

# Colors
RED='\033[1;31m'
GREEN='\033[1;32m'
YELLOW='\033[1;33m'
CYAN='\033[1;36m'
BOLD='\033[1m'
RESET='\033[0m'

ok()    { echo -e "  ${GREEN}[OK]${RESET}  $*"; }
warn()  { echo -e "  ${YELLOW}[WARN]${RESET}  $*"; }
fail()  { echo -e "  ${RED}[FAIL]${RESET}  $*"; }
info()  { echo -e "  ${CYAN}[..]${RESET}  $*"; }

# Helpers
command_exists() { command -v "$1" >/dev/null 2>&1; }

detect_distro() {
    if [[ -f /etc/debian_version ]]; then
        echo "debian"
    elif [[ -f /etc/arch-release ]]; then
        echo "arch"
    elif [[ -f /etc/fedora-release ]] || [[ -f /etc/redhat-release ]]; then
        echo "fedora"
    else
        echo "unknown"
    fi
}

# Banner
echo ""
echo -e "  ${BOLD}${GREEN}isi-music${RESET} ${BOLD}- Linux Installer${RESET}"
echo -e "  $(printf '-%.0s' {1..50})"
echo ""

# Step 1: Audio dependencies
echo -e "  ${BOLD}Step 1/4: Audio dependencies${RESET}"
echo ""

DISTRO=$(detect_distro)
case "$DISTRO" in
    debian)
        info "Detected Debian/Ubuntu — installing libasound2, libpulse0…"
        sudo apt-get update -qq
        sudo apt-get install -y -qq libasound2t64 libpulse0 2>/dev/null || \
        sudo apt-get install -y -qq libasound2 libpulse0
        ok "Audio dependencies installed"
        ;;
    arch)
        info "Detected Arch Linux — installing alsa-lib, libpulse…"
        sudo pacman -S --noconfirm --needed alsa-lib libpulse
        ok "Audio dependencies installed"
        ;;
    fedora)
        info "Detected Fedora — installing alsa-lib, pulseaudio-libs…"
        sudo dnf install -y alsa-lib pulseaudio-libs
        ok "Audio dependencies installed"
        ;;
    *)
        warn "Could not detect distribution. Please install ALSA + PulseAudio manually:"
        warn "  Debian/Ubuntu: sudo apt install libasound2 libpulse0"
        warn "  Arch:          sudo pacman -S alsa-lib libpulse"
        warn "  Fedora:        sudo dnf install alsa-lib pulseaudio-libs"
        ;;
esac
echo ""

# Step 2: Download binary
echo -e "  ${BOLD}Step 2/4: Download isi-music${RESET}"
echo ""

if command_exists "$BINARY_NAME" && [[ "$1" != "--force" ]]; then
    warn "isi-music is already installed at: $(command -v $BINARY_NAME)"
    read -rp "  Reinstall? (y/N) " reinstall
    if [[ ! "$reinstall" =~ ^[Yy] ]]; then
        ok "Keeping existing installation"
        echo ""
        SKIP_DOWNLOAD=1
    fi
fi

if [[ -z "${SKIP_DOWNLOAD:-}" ]]; then
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/isi-music-linux-x86_64"
    TMP_FILE="/tmp/${BINARY_NAME}-download"

    info "Downloading latest release from GitHub…"
    if curl -fSL "$DOWNLOAD_URL" -o "$TMP_FILE"; then
        chmod +x "$TMP_FILE"
        info "Installing to ${INSTALL_DIR}/${BINARY_NAME}…"
        sudo mv "$TMP_FILE" "${INSTALL_DIR}/${BINARY_NAME}"
        ok "isi-music installed to ${INSTALL_DIR}/${BINARY_NAME}"
    else
        fail "Could not download binary. Check your internet connection or visit:"
        fail "  https://github.com/${REPO}/releases"
        exit 1
    fi
fi
echo ""

# Step 2b: Install desktop icon + .desktop file (Linux)
if [[ -z "${SKIP_DOWNLOAD:-}" ]]; then
    ICON_BASE="${HOME}/.local/share/icons/hicolor"
    APPS_DIR="${HOME}/.local/share/applications"

    info "Installing desktop icon and launcher…"

    # Download icon files from the repo
    for size in 16 32 48 128 256 512; do
        ICON_URL="https://raw.githubusercontent.com/${REPO}/main/assets/icons/hicolor/${size}x${size}/apps/isi-music.png"
        ICON_DEST="${ICON_BASE}/${size}x${size}/apps/isi-music.png"
        mkdir -p "$(dirname "$ICON_DEST")"
        curl -fsSL "$ICON_URL" -o "$ICON_DEST" 2>/dev/null || true
    done

    # Download .desktop file
    mkdir -p "$APPS_DIR"
    DESKTOP_URL="https://raw.githubusercontent.com/${REPO}/main/assets/isi-music.desktop"
    curl -fsSL "$DESKTOP_URL" -o "${APPS_DIR}/isi-music.desktop" 2>/dev/null || true

    # Update desktop database (best effort)
    update-desktop-database "$APPS_DIR" 2>/dev/null || true
    ok "Desktop icon and launcher installed"
fi
echo ""

# Step 3: Nerd Font
echo -e "  ${BOLD}Step 3/4: Nerd Font${RESET}"
echo ""

# Check if a Nerd Font is already installed
has_nerd_font() {
    if command_exists fc-list; then
        fc-list 2>/dev/null | grep -qi "nerd" && return 0
    fi
    # Check common font directories
    [[ -d "${FONT_DIR}/nerd-fonts" ]] && return 0
    # Check if user has any Nerd Font in common locations
    find "${HOME}/.local/share/fonts" "${HOME}/.fonts" "/usr/share/fonts" \
         -iname "*nerd*" -type f 2>/dev/null | head -1 | grep -q . && return 0
    return 1
}

if has_nerd_font; then
    ok "Nerd Font already installed"
    SKIP_FONT=1
fi

if [[ -z "${SKIP_FONT:-}" ]]; then
    if ! command_exists fc-cache; then
        warn "fontconfig not found — skipping Nerd Font installation"
        warn "Please install a Nerd Font manually from https://www.nerdfonts.com/"
    else
        info "Downloading ${NERD_FONT_NAME} Nerd Font v${NERD_FONT_VERSION}…"
        mkdir -p "$FONT_DIR"
        FONT_URL="https://github.com/ryanoasis/nerd-fonts/releases/download/v${NERD_FONT_VERSION}/${NERD_FONT_ZIP}"
        TMP_ZIP="/tmp/${NERD_FONT_ZIP}"

        if curl -fSL "$FONT_URL" -o "$TMP_ZIP"; then
            info "Extracting to ${FONT_DIR}…"
            unzip -qo "$TMP_ZIP" -d "$FONT_DIR"
            rm -f "$TMP_ZIP"
            fc-cache -fv >/dev/null 2>&1
            ok "${NERD_FONT_NAME} Nerd Font installed"
            warn "Configure your terminal to use '${NERD_FONT_NAME} Nerd Font'"
        else
            fail "Could not download Nerd Font. Install manually from:"
            fail "  https://www.nerdfonts.com/font-downloads"
        fi
    fi
fi
echo ""

# Step 4: Setup wizard
echo -e "  ${BOLD}Step 4/4: Setup wizard${RESET}"
echo ""

if command_exists "$BINARY_NAME"; then
    info "Launching setup wizard…"
    echo ""
    "$BINARY_NAME" setup || warn "Setup wizard exited with an error. You can re-run it later with: isi-music setup"
else
    warn "isi-music not found on PATH. Skipping setup wizard."
    warn "Run '${BINARY_NAME} setup' after ensuring ${INSTALL_DIR} is on your PATH."
fi
echo ""

# Summary
echo -e "  $(printf '-%.0s' {1..50})"
echo -e "  ${BOLD}${GREEN}Installation complete!${RESET}"
echo ""
echo -e "  Next steps:"
echo -e "    1. Configure your terminal to use a Nerd Font (if not done)"
echo -e "    2. Run ${BOLD}isi-music${RESET} to start playing"
echo -e "    3. Run ${BOLD}isi-music doctor${RESET} if something isn't working"
echo ""
