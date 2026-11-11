#!/usr/bin/env bash
set -euo pipefail
# Ubuntu 24.04 is the supported build/ABI baseline on both native architectures.
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
  libpipewire-0.3-dev libspa-0.2-dev libdbus-1-dev libsecret-1-dev \
  libssl-dev libgbm-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan1 \
  cmake clang libclang-dev patchelf file dpkg-dev squashfs-tools \
  dbus-x11 xvfb xauth fonts-noto-cjk libfuse2t64 libopus0 pkexec polkitd
