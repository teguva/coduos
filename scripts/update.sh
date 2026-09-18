#!/usr/bin/env bash
# Update an existing CoduOS install from the latest GitHub Release.
# Replaces the daemon, web UI, and unit files. Keeps config and data.
# Compares packaging/deps against installed packages and offers to install missing ones.
# Does not install or change Docker, nginx, or WireGuard.
#
#   curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/update.sh | sudo bash
#   CODUOS_PACKAGES=no sudo -E bash scripts/update.sh   # skip missing packages
set -euo pipefail

REPO="${CODUOS_REPO:-teguva/coduos}"
VERSION="${CODUOS_VERSION:-latest}"
LOCAL_TARBALL="${1:-}"

if [[ ${EUID} -ne 0 ]]; then
  echo "Run as root: sudo bash scripts/update.sh" >&2
  exit 1
fi

if [[ ! -x /usr/bin/coduosd ]]; then
  echo "CoduOS is not installed. First-time install:" >&2
  echo "  curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/install.sh | sudo bash" >&2
  exit 1
fi

arch=$(uname -m)
case "${arch}" in
  x86_64|amd64) TARGET_ARCH=amd64 ;;
  aarch64|arm64) TARGET_ARCH=arm64 ;;
  armv7l|armv7) TARGET_ARCH=arm-7 ;;
  *) echo "unsupported architecture: ${arch}" >&2; exit 1 ;;
esac

if command -v apt-get >/dev/null 2>&1; then
  PM=apt
elif command -v pacman >/dev/null 2>&1; then
  PM=pacman
elif command -v dnf >/dev/null 2>&1; then
  PM=dnf
elif command -v apk >/dev/null 2>&1; then
  PM=apk
else
  PM=unknown
fi

pkg_install() {
  if [[ $# -eq 0 ]]; then
    return 0
  fi
  case "${PM}" in
    apt)
      export DEBIAN_FRONTEND=noninteractive
      if [[ "${APT_UPDATED:-0}" -ne 1 ]]; then
        apt-get update -qq
        APT_UPDATED=1
      fi
      apt-get install -y "$@"
      ;;
    pacman)
      pacman -Sy --noconfirm --needed "$@"
      ;;
    dnf)
      dnf install -y "$@"
      ;;
    apk)
      apk add --no-cache "$@"
      ;;
    *)
      echo "No supported package manager. Install manually: $*" >&2
      return 1
      ;;
  esac
}

ask_yes() {
  local prompt="$1"
  local default="${2:-y}"
  local preset="${3:-}"
  local reply=""

  if [[ -n "${preset}" ]]; then
    case "${preset,,}" in
      y|yes|1|true|on) return 0 ;;
      n|no|0|false|off) return 1 ;;
    esac
  fi

  if [[ ! -r /dev/tty ]]; then
    if [[ "${default}" == y ]]; then
      echo "${prompt} -> yes (no TTY; override with CODUOS_PACKAGES=no)"
      return 0
    fi
    echo "${prompt} -> no (no TTY)"
    return 1
  fi

  if [[ "${default}" == y ]]; then
    read -r -p "${prompt} [Y/n] " reply </dev/tty || true
    reply="${reply:-y}"
  else
    read -r -p "${prompt} [y/N] " reply </dev/tty || true
    reply="${reply:-n}"
  fi
  [[ "${reply}" =~ ^[Yy] ]]
}

fallback_deps() {
  cat <<'EOF'
parted|parted|Format a whole disk
e2fsprogs|mkfs.ext4|Format ext4 volumes
openssl|openssl|HTTPS certificates and LAN CA
EOF
}

if ! command -v curl >/dev/null 2>&1 || ! command -v tar >/dev/null 2>&1; then
  echo "curl and tar are required" >&2
  exit 1
fi

before=""
if command -v /usr/bin/coduosd >/dev/null 2>&1; then
  before=$(/usr/bin/coduosd --version 2>/dev/null | awk '{print $NF}' || true)
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "${tmpdir}"' EXIT

if [[ -n "${LOCAL_TARBALL}" ]]; then
  echo "Updating CoduOS from ${LOCAL_TARBALL}"
  tar -xzf "${LOCAL_TARBALL}" -C "${tmpdir}"
  ver="local"
else
  if [[ "${VERSION}" == latest ]]; then
    api="https://api.github.com/repos/${REPO}/releases/latest"
    tag=$(curl -fsSL -H "Accept: application/vnd.github+json" "${api}" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
    if [[ -z "${tag}" ]]; then
      echo "Could not resolve latest release from ${api}" >&2
      exit 1
    fi
  else
    tag="${VERSION}"
  fi
  ver="${tag#v}"
  url="https://github.com/${REPO}/releases/download/${tag}/linux-${TARGET_ARCH}-coduos-v${ver}.tar.gz"
  echo "Updating CoduOS to ${tag}"
  echo "Downloading ${url}"
  curl -fL --progress-bar -o "${tmpdir}/coduos.tar.gz" "${url}"
  tar -xzf "${tmpdir}/coduos.tar.gz" -C "${tmpdir}"
fi

root="${tmpdir}"
if [[ -d "${tmpdir}/usr" ]]; then
  root="${tmpdir}"
elif [[ -d "${tmpdir}/linux-${TARGET_ARCH}-coduos-"* ]]; then
  root=$(echo "${tmpdir}"/linux-${TARGET_ARCH}-coduos-*)
fi

if [[ ! -x "${root}/usr/bin/coduosd" ]]; then
  echo "tarball is missing usr/bin/coduosd" >&2
  exit 1
fi

deps_text=""
if [[ -f "${root}/usr/share/coduos/deps" ]]; then
  deps_text=$(cat "${root}/usr/share/coduos/deps")
else
  deps_text=$(fallback_deps)
fi
missing_pkgs=()
echo
echo "Checking packages…"
while IFS='|' read -r pkg cmd reason; do
  pkg="${pkg#"${pkg%%[![:space:]]*}"}"
  [[ -z "${pkg}" || "${pkg}" == \#* ]] && continue
  cmd="${cmd#"${cmd%%[![:space:]]*}"}"
  reason="${reason#"${reason%%[![:space:]]*}"}"
  if command -v "${cmd}" >/dev/null 2>&1; then
    echo "  ${pkg}: installed"
  else
    echo "  ${pkg}: missing — ${reason}"
    missing_pkgs+=("${pkg}")
  fi
done <<< "${deps_text}"
if [[ ${#missing_pkgs[@]} -gt 0 ]]; then
  echo "Missing: ${missing_pkgs[*]}"
  if ask_yes "Install missing packages?" y "${CODUOS_PACKAGES:-}"; then
    pkg_install "${missing_pkgs[@]}" || true
  else
    echo "Skipping package install."
  fi
else
  echo "All listed packages are installed."
fi

install -d /usr/bin /usr/share/coduos /usr/lib/systemd/system
install -m 0755 "${root}/usr/bin/coduosd" /usr/bin/coduosd
rm -rf /usr/share/coduos/www
cp -a "${root}/usr/share/coduos/www" /usr/share/coduos/www
if [[ -f "${root}/usr/share/coduos/deps" ]]; then
  install -m 0644 "${root}/usr/share/coduos/deps" /usr/share/coduos/deps
fi
install -m 0644 "${root}/usr/lib/systemd/system/coduosd.service" /usr/lib/systemd/system/coduosd.service
if [[ -f "${root}/usr/lib/systemd/system/coduos-wg.service" ]]; then
  install -m 0644 "${root}/usr/lib/systemd/system/coduos-wg.service" /usr/lib/systemd/system/coduos-wg.service
fi

systemctl daemon-reload
systemctl enable coduosd.service
if ! systemctl restart coduosd.service; then
  systemctl start coduosd.service
fi
if command -v nginx >/dev/null 2>&1 && [[ -f /etc/nginx/conf.d/coduos.conf || -d /var/lib/coduos/nginx/conf.d ]]; then
  systemctl enable nginx.service 2>/dev/null || systemctl enable nginx 2>/dev/null || true
  systemctl start nginx.service 2>/dev/null || systemctl start nginx 2>/dev/null || true
fi

after=$(/usr/bin/coduosd --version 2>/dev/null | awk '{print $NF}' || echo "${ver}")
echo
if [[ -n "${before}" && -n "${after}" && "${before}" != "${after}" ]]; then
  echo "CoduOS updated: ${before} → ${after}"
else
  echo "CoduOS updated to ${after:-${ver}}"
fi
echo "Config and data were left in /etc/coduos and /var/lib/coduos."
