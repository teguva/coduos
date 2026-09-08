#!/usr/bin/env bash
# Build web + daemon and pack linux-${arch}-coduos-v${version}.tar.gz
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "${root}"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
case "${arch}" in
  x86_64|amd64) TARGET_ARCH=amd64 ;;
  aarch64|arm64) TARGET_ARCH=arm64 ;;
  armv7l|armv7) TARGET_ARCH=arm-7 ;;
  *) echo "unsupported architecture: ${arch}" >&2; exit 1 ;;
esac

(cd web && npm ci && npm run build)
cargo build --release -p coduosd

stage=$(mktemp -d)
trap 'rm -rf "${stage}"' EXIT
install -d "${stage}/usr/bin" "${stage}/usr/share/coduos" "${stage}/usr/lib/systemd/system" "${stage}/etc/coduos"
install -m 0755 "${root}/target/release/coduosd" "${stage}/usr/bin/coduosd"
cp -a "${root}/web/dist" "${stage}/usr/share/coduos/www"
install -m 0644 "${root}/packaging/coduosd.service" "${stage}/usr/lib/systemd/system/coduosd.service"
install -m 0644 "${root}/packaging/coduos.toml" "${stage}/etc/coduos/coduos.toml"

mkdir -p "${root}/dist"
tarball="${root}/dist/linux-${TARGET_ARCH}-coduos-v${version}.tar.gz"
tar -C "${stage}" -czf "${tarball}" usr etc
echo "Wrote ${tarball}"
