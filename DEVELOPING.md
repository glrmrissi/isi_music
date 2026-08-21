# Developing isi-music

This document is for the ~1% of people building or contributing to isi-music.
End users only need the [README](README.md).

## Prerequisites

Requires Rust 1.88 or newer (`rustup` recommended) and Git.

**Linux dependencies:**

```bash
sudo apt install libasound2-dev libpulse-dev libdbus-1-dev pkg-config cmake
```

**Windows:** MSVC Build Tools and CMake. The bundled Opus build uses CMake on
Linux and Windows.

## Build and run

```bash
git clone https://github.com/glrmrissi/isi_music.git
cd isi_music
cargo build --release
cargo run --release
```

Debug builds are faster to iterate with (`cargo run`), but the release profile
is heavily optimized (`opt-level = "z"`, fat LTO).

## Feature flags

The default build enables Spotify, Discord, album art, and palette (reactive
theming). MPRIS is optional on Linux.

```bash
cargo build --release --no-default-features -F spotify,discord
cargo build --release --no-default-features -F spotify,discord,mpris
cargo build --release --no-default-features -F spotify,album-art
```

Available features: `spotify`, `discord`, `album-art`, `palette`, `mpris`.

## Verification

Run these before committing or opening a PR:

```bash
cargo fmt --check
cargo check --all-targets --all-features --locked
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo deny --all-features --locked check
```

The test suite (197 tests) covers the app state machine, keybindings, and the
UI. Tests use an in-memory SQLite database and `App::new_for_test()`, never
real user paths.

### Local CI scripts

| Script | Purpose |
| --- | --- |
| `tools/ci/windows/ci-all.ps1` | Local pipeline for Windows in 3 modes: `dev` (fmt + tests, ~5s), `pre-push` (+ clippy + release builds), `ci` (+ Linux x64 + ARM64 via WSL2) |
| `tools/ci/linux/ci-all.sh` | Same pipeline for native Linux (`ci` mode adds ARM64 cross-compile with the native toolchain) |
| `tools/ci/windows/ci-local.ps1` | Emulates the GitHub Actions pipeline (fmt, deny, clippy, tests, release builds) |
| `tools/ci/windows/ci-wsl.sh` | Cross-compiles Linux x64/ARM64 inside WSL2 without Docker |
| `tools/dev/clean-cache.sh` | Clears Cargo, cross, and Docker build caches |
| `tools/dev/setup-wsl-docker.sh` | One-time WSL2 setup: Docker Engine + cross |

Machine-specific paths are overridable: `ISI_BUILD_DIR` (default `/mnt/e/isi-music-build`), `ISI_CARGO_CACHE` (default `/mnt/e/cargo-cache`), `ISI_DOCKER_DATA_ROOT` (default `/mnt/e/docker-data`), and `-WslDistro` on `ci-all.ps1` (default `Debian`).

## Git hooks

Hooks live in `.githooks/` and are enabled with:

```bash
git config core.hooksPath .githooks
```

| Hook | Action |
| --- | --- |
| pre-commit | `cargo fmt --check` + `cargo clippy -- -D warnings` (set `HOOK_SKIP_CLIPPY=1` to skip clippy) |
| pre-push | `cargo test` (set `HOOK_SKIP_TEST=1` to skip) |
| commit-msg | validates Conventional Commits (single line, types: feat/fix/chore/perf/docs/refactor/style/test/build/ci/revert) |

## Cross-compiling for Linux ARM64

### Via `cross` (Docker)

```bash
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu
```

`Cross.toml` and `docker/Dockerfile.aarch64-unknown-linux-gnu` provide the
container with the ARM64 ALSA development files.

### Via WSL2 (native toolchain, no Docker)

```bash
# One-time setup (Debian/Ubuntu)
sudo dpkg --add-architecture arm64
sudo apt update
sudo apt install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libasound2-dev:arm64 libc6-dev:arm64
rustup target add aarch64-unknown-linux-gnu

# Build (from the project root inside WSL)
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig
export PKG_CONFIG_SYSROOT_DIR=/
cargo build --release --target aarch64-unknown-linux-gnu
```

`tools/ci/windows/ci-wsl.sh arm64` automates all of the above (including
syncing the project out of `/mnt/c`, where I/O over 9p is extremely slow).

The GitHub release workflow builds ARM64 the same way (native cross-compile on
`ubuntu-latest`, with ARM64 apt sources pinned to `ports.ubuntu.com`).

### TLS requirements

The dependency tree must stay free of `native-tls` and `openssl-sys`; they
break cross-compilation. Verify with:

```bash
cargo tree -i native-tls
cargo tree -i openssl-sys
```

Both must report "did not match any packages".

## Patches

The project vendors two crates under `patches/` (wired via
`[patch.crates-io]` in `Cargo.toml`):

**`patches/librespot-core`** is pinned to the crates.io `0.8.0` source with four deliberate changes: quick-xml 0.41 API fix, keymaster fallback when the login5 token endpoint fails, extra `warn!` logging on failed internal requests, and a vendored build script replacing `vergen` with static build info.

**`patches/hyper-proxy2`** is identical to upstream `0.1.0` source, with `Cargo.toml` updated to rustls 0.23 / hyper-rustls 0.27 using the `ring` crypto provider (no `aws-lc-rs`, which avoids a heavy C build for ARM64).

When touching patches: document what was changed and why in the patch directory; when upgrading a patched crate, re-apply the changes manually and re-check the TLS tree (`cargo tree -i native-tls` and `-i openssl-sys`); keep `rustls` on the `ring` provider, because `aws-lc-rs` needs CMake toolchain setup for every cross-compile target.
