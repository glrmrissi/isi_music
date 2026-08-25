#!/bin/bash
# ci-all.sh: Local CI pipeline for native Linux (mirror of ci-all.ps1 for Windows).
# Modes: dev (fast), pre-push (medium), ci (full, adds ARM64 cross-compile).
#
# Usage:
#   bash tools/ci/linux/ci-all.sh                    # dev: fmt + tests
#   bash tools/ci/linux/ci-all.sh pre-push           # + clippy + release builds
#   bash tools/ci/linux/ci-all.sh ci                 # + ARM64 cross-compile
#   bash tools/ci/linux/ci-all.sh dev --clean        # clean cache before running
#   bash tools/ci/linux/ci-all.sh ci --skip-arm      # ci without ARM64

set -euo pipefail

# Ensure the Rust toolchain is on PATH even when invoked without a login shell
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

MODE="${1:-dev}"
CLEAN=false
SKIP_ARM=false
for arg in "$@"; do
    case "$arg" in
        --clean) CLEAN=true ;;
        --skip-arm) SKIP_ARM=true ;;
    esac
done

# Resolve the project root from this script's location
PROJECT_SRC="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$PROJECT_SRC"

# Pin the target dir so ambient cargo configs (e.g. a Windows global config
# with a drive-letter target-dir) cannot break the Linux build
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PROJECT_SRC/target}"

PASS=0
FAIL=0
SKIPPED=0

run_step() {
    local label="$1"
    shift
    echo "  [RUN]  $label"
    if "$@" > /tmp/ci-all-step.log 2>&1; then
        echo "  [PASS] $label"
        PASS=$((PASS + 1))
    else
        tail -5 /tmp/ci-all-step.log | sed 's/^/      /'
        echo "  [FAIL] $label"
        FAIL=$((FAIL + 1))
    fi
}

# Detect nextest
HAS_NEXTEST=0
command -v cargo-nextest >/dev/null 2>&1 && HAS_NEXTEST=1

# Clean
if [ "$CLEAN" = true ]; then
    echo "-- Cleaning cache --"
    cargo clean 2>&1
    echo "  cargo clean done"
fi

echo "-- Mode: $MODE --"

# fmt --check (always, fast)
run_step "cargo fmt --check" cargo fmt --check

# test (nextest when available, otherwise cargo test)
if [ "$HAS_NEXTEST" = 1 ]; then
    run_step "cargo nextest run" cargo nextest run --locked
else
    run_step "cargo test --locked" cargo test --locked
fi

# PRE-PUSH MODE: + clippy + release builds
if [ "$MODE" = "pre-push" ] || [ "$MODE" = "ci" ]; then
    run_step "cargo clippy -- -D warnings" cargo clippy --all-targets --all-features --locked -- -D warnings
    run_step "cargo build --release" cargo build --release --locked
    run_step "cargo build --release -F mpris" cargo build --release --locked -F mpris
    run_step "cargo build --release (minimal)" cargo build --release --locked --no-default-features -F spotify,discord
fi

# CI MODE: + ARM64 cross-compile (native toolchain, no Docker)
if [ "$MODE" = "ci" ] && [ "$SKIP_ARM" = false ]; then
    echo "-- Linux ARM64 (cross-compile) --"
    if ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
        echo "  [FAIL] aarch64-linux-gnu-gcc not found (install gcc-aarch64-linux-gnu)"
        FAIL=$((FAIL + 1))
    elif ! rustup target list --installed 2>/dev/null | grep -q "^aarch64-unknown-linux-gnu$"; then
        echo "  [FAIL] rust target aarch64-unknown-linux-gnu not installed (rustup target add aarch64-unknown-linux-gnu)"
        FAIL=$((FAIL + 1))
    else
        export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
        export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
        export PKG_CONFIG_ALLOW_CROSS=1
        export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig
        export PKG_CONFIG_SYSROOT_DIR=/
        run_step "cargo build --release (aarch64-linux)" cargo build --release --locked --target aarch64-unknown-linux-gnu
        echo "  [verify] binary:"
        readelf -h "target/aarch64-unknown-linux-gnu/release/isi-music" 2>&1 | grep -E 'Machine|Class' | sed 's/^/    /' || echo "    (binary not found)"
    fi
fi

# Report
echo ""
echo "==========================================================="
echo "  CI REPORT - isi_music [$MODE]"
echo "==========================================================="
echo "  PASS:     $PASS"
echo "  FAIL:     $FAIL"
echo "  SKIPPED:  $SKIPPED"
echo ""

if [ "$FAIL" -gt 0 ]; then
    echo "  RESULT: FAILED - fix errors before pushing"
    exit 1
else
    echo "  RESULT: ALL PASSED - safe to push"
    exit 0
fi
