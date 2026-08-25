#!/bin/bash
# ci-wsl.sh: Cross-compile inside WSL2 without Docker (native toolchain).
# The project copy, target dir, and cargo cache live in ISI_BUILD_DIR
# (default: /mnt/e/isi-music-build; override with the env var).
#
# Usage: bash tools/ci/windows/ci-wsl.sh [target] [--clean] [--build-only]
# Targets: arm64 (default), x64, all

set -e

# Ensure the Rust toolchain is on PATH even when invoked without a login shell
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

TARGET="${1:-arm64}"
CLEAN=false
BUILD_ONLY=false
for arg in "$@"; do
    case "$arg" in
        --clean) CLEAN=true ;;
        --build-only) BUILD_ONLY=true ;;
    esac
done

ISI_BUILD_DIR="${ISI_BUILD_DIR:-/mnt/e/isi-music-build}"

# Resolve the project root from this script's location (/mnt/c/... when run from Windows)
PROJECT_SRC="$(cd "$(dirname "$0")/../../.." && pwd)"

# Sync the project to the build dir (I/O over 9p from /mnt/c is extremely slow)
echo "[setup] Syncing project to $ISI_BUILD_DIR..."
rm -rf "$ISI_BUILD_DIR"
mkdir -p "$ISI_BUILD_DIR"
cp -a "$PROJECT_SRC/." "$ISI_BUILD_DIR/"
rm -rf "$ISI_BUILD_DIR/target" "$ISI_BUILD_DIR/.git"
cd "$ISI_BUILD_DIR"

# Keep the target dir next to the build copy and disable sccache (not installed in WSL)
export CARGO_TARGET_DIR="$ISI_BUILD_DIR/target"
unset RUSTC_WRAPPER

if $CLEAN; then
    echo "[clean] cargo clean..."
    cargo clean 2>&1
fi

run_build() {
    local target=$1
    local label=$2
    echo ""
    echo "=== $label ==="
    echo ""

    # Cross-compile environment
    export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
    export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig
    export PKG_CONFIG_SYSROOT_DIR=/

    echo "[build] cargo build --release --target $target..."
    cargo build --release --locked --target "$target" 2>&1
    echo "[build] OK: $label"

    if $BUILD_ONLY; then
        echo "[test] skipped (--build-only)"
    else
        echo ""
        echo "[test] cargo test --no-run --target $target..."
        # --no-run compiles the test binaries without executing them, so
        # compile errors fail the build while the run step is skipped
        # (aarch64 binaries cannot execute on an x86_64 host).
        cargo test --locked --no-run --target "$target" 2>&1
        echo "[test] compiled OK; execution skipped (cross-compiled tests cannot run on the host)"
    fi

    # Verify the binary architecture
    echo ""
    echo "[verify] binary:"
    readelf -h "target/$target/release/isi-music" 2>&1 | grep -E 'Machine|Class' || echo "  (binary not found)"
}

case "$TARGET" in
    arm64|aarch64)
        run_build "aarch64-unknown-linux-gnu" "Linux ARM64"
        ;;
    x64|x86_64)
        echo ""
        echo "=== Linux x86_64 (native WSL2) ==="
        echo ""
        echo "[build] cargo build --release..."
        cargo build --release --locked 2>&1
        echo "[build] OK"
        echo ""
        echo "[test] cargo test..."
        cargo test --locked 2>&1
        ;;
    all)
        echo ""
        echo "=== Linux x86_64 (native WSL2) ==="
        cargo build --release --locked 2>&1
        cargo test --locked 2>&1
        run_build "aarch64-unknown-linux-gnu" "Linux ARM64"
        ;;
    *)
        echo "Unknown target: $TARGET"
        echo "Usage: bash ci-wsl.sh [arm64|x64|all] [--clean] [--build-only]"
        exit 1
        ;;
esac

echo ""
echo "=== Done ==="
echo "Binaries in: $ISI_BUILD_DIR/target/*/release/isi-music"
