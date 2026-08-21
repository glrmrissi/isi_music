#!/bin/bash
# clean-cache.sh: Clear all build caches (cargo + Docker + cross)
# Override ISI_BUILD_DIR / ISI_CARGO_CACHE to match ci-wsl.sh.
# Usage: bash tools/dev/clean-cache.sh
set -e

# Ensure the Rust toolchain is on PATH even when invoked without a login shell
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

ISI_BUILD_DIR="${ISI_BUILD_DIR:-/mnt/e/isi-music-build}"
ISI_CARGO_CACHE="${ISI_CARGO_CACHE:-/mnt/e/cargo-cache}"

# Resolve the project root from this script's location
PROJECT_SRC="$(cd "$(dirname "$0")/../.." && pwd)"

# Pin the target dir so ambient cargo configs cannot break the clean
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PROJECT_SRC/target}"

echo "=== Cleaning all build cache ==="

# 1. Cargo target (Windows + WSL)
echo "[1/4] cargo clean (Windows target)..."
cd "$PROJECT_SRC"
cargo clean 2>&1 || true

# 2. Cross target in the build dir
echo "[2/4] cross clean ($ISI_BUILD_DIR)..."
cd "$ISI_BUILD_DIR" 2>/dev/null && {
    export CARGO_HOME="$ISI_CARGO_CACHE"
    cross clean 2>&1 || cargo clean 2>&1 || true
} || echo "  no build dir yet"

# 3. Docker builder cache
echo "[3/4] docker builder prune..."
docker builder prune -f 2>&1 || true

# 4. Docker dangling images
echo "[4/4] docker image prune..."
docker image prune -f 2>&1 || true

echo ""
echo "=== Done ==="
echo "Disk usage:"
df -h "$(dirname "$ISI_BUILD_DIR")" 2>&1 | tail -1
