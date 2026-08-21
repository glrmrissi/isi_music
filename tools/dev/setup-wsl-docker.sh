#!/bin/bash
# setup-wsl-docker.sh: Install Docker Engine + cross inside WSL2 Debian
# Docker data-root defaults to /mnt/e/docker-data (override with ISI_DOCKER_DATA_ROOT).
# Run from inside WSL: bash setup-wsl-docker.sh
set -e

ISI_DOCKER_DATA_ROOT="${ISI_DOCKER_DATA_ROOT:-/mnt/e/docker-data}"

echo "=== Installing Docker Engine in WSL2 ==="

# 1. Docker Engine
if ! command -v docker &>/dev/null; then
  echo "[1/3] Installing Docker Engine..."
  sudo apt-get update
  sudo apt-get install -y ca-certificates curl
  sudo install -m 0755 -d /etc/apt/keyrings
  sudo curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
  sudo chmod a+r /etc/apt/keyrings/docker.asc

  echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

  sudo apt-get update
  sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

  # Keep Docker data on the E: drive (via /mnt/e)
  echo "  Configuring Docker data-root to /mnt/e/docker-data..."
  sudo mkdir -p "$ISI_DOCKER_DATA_ROOT"
  sudo tee /etc/docker/daemon.json > /dev/null <<EOF
{
  "data-root": "$ISI_DOCKER_DATA_ROOT"
}
EOF

  # Start the daemon (WSL2 has no systemd by default)
  sudo service docker start

  # Allow the current user to run docker without sudo
  sudo usermod -aG docker $USER
  echo "  Docker installed. You may need to re-login for group changes."
else
  echo "[1/3] Docker already installed: $(docker --version)"
  # Ensure the data-root config exists
  if [ ! -f /etc/docker/daemon.json ]; then
    echo "  Configuring Docker data-root to /mnt/e/docker-data..."
    sudo mkdir -p "$ISI_DOCKER_DATA_ROOT"
    sudo tee /etc/docker/daemon.json > /dev/null <<EOF
{
  "data-root": "$ISI_DOCKER_DATA_ROOT"
}
EOF
  fi
  sudo service docker start 2>/dev/null || true
fi

# 2. Rust targets
echo "[2/3] Adding Rust targets..."
source "$HOME/.cargo/env" 2>/dev/null || true
rustup target add x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu

# 3. cross
echo "[3/3] Installing cross..."
if ! command -v cross &>/dev/null; then
  cargo install cross
else
  echo "  cross already installed: $(cross --version)"
fi

echo ""
echo "=== Setup complete ==="
echo ""
echo "Verify:"
echo "  docker run hello-world"
echo "  cross --version"
echo ""
echo "If docker permission denied, run: newgrp docker"
