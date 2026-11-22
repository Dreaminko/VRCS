#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "Usage: $0 <deb|rpm> <version>" >&2
  exit 1
fi

format="$1"
version="$2"
pkg="/artifacts/VRCS-${version}-linux-x86_64.${format}"

if [ ! -f "$pkg" ]; then
  echo "Missing package: $pkg" >&2
  exit 1
fi

case "$format" in
  deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends ca-certificates "$pkg"
    ;;
  rpm)
    if command -v dnf >/dev/null 2>&1; then
      dnf install -y --setopt=localpkg_gpgcheck=0 --setopt=install_weak_deps=False ca-certificates "$pkg"
    elif command -v zypper >/dev/null 2>&1; then
      zypper --non-interactive refresh
      zypper --non-interactive install --no-recommends --allow-unsigned-rpm ca-certificates ca-certificates-mozilla "$pkg"
    else
      echo "Unsupported RPM distribution: neither dnf nor zypper found" >&2
      exit 1
    fi
    ;;
  *)
    echo "Unsupported package format: $format" >&2
    exit 1
    ;;
esac

bin="/usr/bin/vrcs-desktop"
policy="/usr/share/polkit-1/actions/app.vrcs.network-capture.policy"

[ -f "$bin" ] || { echo "Missing $bin" >&2; exit 1; }
bin_stat="$(stat -c '%u:%g %a' "$bin")"
if [ "$bin_stat" != "0:0 755" ]; then
  echo "Unexpected ownership/mode on $bin: $bin_stat" >&2
  exit 1
fi

[ -f "$policy" ] || { echo "Missing $policy" >&2; exit 1; }
policy_stat="$(stat -c '%u:%g %a' "$policy")"
if [ "$policy_stat" != "0:0 644" ]; then
  echo "Unexpected ownership/mode on $policy: $policy_stat" >&2
  exit 1
fi

if [ ! -f /usr/bin/pkexec ]; then
  echo "Missing /usr/bin/pkexec" >&2
  exit 1
fi

ldd_out="$(ldd "$bin")"
printf '%s\n' "$ldd_out"
if printf '%s\n' "$ldd_out" | grep -q 'not found'; then
  echo "Unresolved dynamic dependencies in $bin" >&2
  exit 1
fi

ldconfig_out="$(PATH="$PATH:/sbin:/usr/sbin" ldconfig -p)"
for soname in libopus.so.0 libvulkan.so.1 libssl.so.3 libgomp.so.1; do
  if ! printf '%s\n' "$ldconfig_out" | grep -Fq "$soname"; then
    echo "Missing shared library in ldconfig cache: $soname" >&2
    exit 1
  fi
done

helper_version="$("$bin" --network-helper --version)"
printf '%s\n' "$helper_version"
case "$helper_version" in
  *vrcs-network-helper*) ;;
  *)
    echo "Unexpected --network-helper --version output: $helper_version" >&2
    exit 1
    ;;
esac

LD_LIBRARY_PATH= "$bin" --release-self-test
