#!/usr/bin/env bash
set -euo pipefail
# Ubuntu 24.04 (glibc 2.39) is the CI/release build/ABI baseline (compatible with Ubuntu 24.04+, Debian 13+, Fedora, and openSUSE Tumbleweed).
CI_MODE=0
for arg in "$@"; do
  case "$arg" in
    --ci) CI_MODE=1 ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 1
      ;;
  esac
done
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  SUDO="sudo"
fi
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update
libssl_pkg=libssl3
if apt-cache show libssl3t64 2>/dev/null | grep -q '^Package:'; then
  libssl_pkg=libssl3t64
fi
libfuse_pkg=libfuse2
if apt-cache show libfuse2t64 2>/dev/null | grep -q '^Package:'; then
  libfuse_pkg=libfuse2t64
fi
extra_pkg=""
if [ "$CI_MODE" -eq 0 ]; then
  extra_pkg="fonts-noto-cjk"
fi
$SUDO apt-get install -y --no-install-recommends \
  ca-certificates curl xz-utils \
  build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
  libpipewire-0.3-dev libspa-0.2-dev libdbus-1-dev libsecret-1-dev \
  libssl-dev "$libssl_pkg" libgomp1 libgbm-dev libegl1-mesa-dev libgl1-mesa-dev libvulkan1 \
  cmake clang libclang-dev patchelf file dpkg-dev rpm rpm2cpio cpio squashfs-tools \
  dbus-x11 xvfb xauth $extra_pkg "$libfuse_pkg" libopus0 pkexec polkitd
