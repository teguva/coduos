#!/usr/bin/env bash
# Update an existing CoduOS install from the latest GitHub Release.
# Replaces the daemon, web UI, and unit files. Keeps config and data.
# Compares packaging/deps against installed packages and offers to install missing ones.
# Does not enable Docker, nginx, or WireGuard if they were skipped at install.
# Installs missing listed deps and upgrades those apt packages.
# If Docker Compose is older than 2.29, installs Docker's plugin (for --progress json).
#
#   curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/update.sh | sudo bash
#   CODUOS_PACKAGES=no sudo -E bash scripts/update.sh   # skip missing packages
#   CODUOS_APT_UPGRADE=no sudo -E bash scripts/update.sh  # skip apt upgrade of listed packages
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

# Try candidate package names until one installs.
pkg_install_any() {
  local candidate
  for candidate in "$@"; do
    if pkg_install "${candidate}"; then
      return 0
    fi
  done
  echo "Could not install any of: $*" >&2
  return 1
}

# App installs pass `docker compose --progress json` (Compose 2.29+). Debian's
# docker-compose 2.26 rejects that flag, so distro packages are not enough.
COMPOSE_MIN="2.29.0"
COMPOSE_PLUGIN_VERSION="${CODUOS_COMPOSE_VERSION:-v2.40.3}"

compose_short_version() {
  docker compose version --short 2>/dev/null | sed -E 's/^v//; s/[-+].*$//' || true
}

# $1 >= $2 as dotted numbers (2.26.1 vs 2.29.0).
version_ge() {
  local IFS=.
  local -a a=() b=()
  local i x y
  read -r -a a <<<"${1}"
  read -r -a b <<<"${2}"
  for i in 0 1 2; do
    x="${a[i]:-0}"
    y="${b[i]:-0}"
    x="${x%%[^0-9]*}"
    y="${y%%[^0-9]*}"
    x="${x:-0}"
    y="${y:-0}"
    if ((10#$x > 10#$y)); then
      return 0
    fi
    if ((10#$x < 10#$y)); then
      return 1
    fi
  done
  return 0
}

compose_asset_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo x86_64 ;;
    aarch64|arm64) echo aarch64 ;;
    armv7l|armv7) echo armv7 ;;
    *) return 1 ;;
  esac
}

install_compose_plugin() {
  local arch dest tmp url ver
  arch=$(compose_asset_arch) || {
    echo "warning: no Docker Compose plugin for $(uname -m)" >&2
    return 1
  }
  ver="${COMPOSE_PLUGIN_VERSION}"
  url="https://github.com/docker/compose/releases/download/${ver}/docker-compose-linux-${arch}"
  dest=/usr/local/lib/docker/cli-plugins
  echo "Installing Docker Compose ${ver} (${arch}) for app install progress…"
  install -d "${dest}"
  tmp=$(mktemp)
  if ! curl -fL --progress-bar -o "${tmp}" "${url}"; then
    rm -f "${tmp}"
    echo "Could not download ${url}" >&2
    return 1
  fi
  chmod 0755 "${tmp}"
  mv -f "${tmp}" "${dest}/docker-compose"
}

ensure_docker_compose() {
  if ! command -v docker >/dev/null 2>&1; then
    return 0
  fi
  local have
  have=$(compose_short_version)
  if [[ -n "${have}" ]] && version_ge "${have}" "${COMPOSE_MIN}"; then
    echo "Docker Compose ${have} (>= ${COMPOSE_MIN})"
    return 0
  fi
  if [[ -n "${have}" ]]; then
    echo "Docker Compose ${have} is too old for app installs (need ${COMPOSE_MIN}+)."
  else
    echo "Docker Compose plugin is missing."
  fi
  if ! install_compose_plugin; then
    echo "Apps need Docker Compose ${COMPOSE_MIN}+ (--progress json). Distro packages are not new enough." >&2
    return 1
  fi
  have=$(compose_short_version)
  echo "Docker Compose ${have:-unknown}"
  if [[ -z "${have}" ]] || ! version_ge "${have}" "${COMPOSE_MIN}"; then
    echo "Docker Compose is still older than ${COMPOSE_MIN}" >&2
    return 1
  fi
}

dep_ok() {
  local cmd="$1"
  local bin rest
  read -r bin rest <<<"${cmd}"
  if [[ -z "${rest}" ]]; then
    command -v "${bin}" >/dev/null 2>&1
  else
    "${bin}" ${rest} >/dev/null 2>&1
  fi
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
docker-compose-v2,docker-compose-plugin,docker-compose,docker-cli-compose|docker compose version|Docker Compose 2.29+ for app installs
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
all_specs=()
echo
echo "Checking packages…"
while IFS='|' read -r pkg cmd reason; do
  pkg="${pkg#"${pkg%%[![:space:]]*}"}"
  [[ -z "${pkg}" || "${pkg}" == \#* ]] && continue
  cmd="${cmd#"${cmd%%[![:space:]]*}"}"
  reason="${reason#"${reason%%[![:space:]]*}"}"
  IFS=',' read -r -a alts <<<"${pkg}"
  all_specs+=("${pkg}")
  if dep_ok "${cmd}"; then
    echo "  ${alts[0]}: installed"
  else
    echo "  ${alts[0]}: missing — ${reason}"
    missing_pkgs+=("${pkg}")
  fi
done <<< "${deps_text}"
if [[ ${#missing_pkgs[@]} -gt 0 ]]; then
  echo "Missing: ${missing_pkgs[*]}"
  if ask_yes "Install missing packages?" y "${CODUOS_PACKAGES:-}"; then
    for spec in "${missing_pkgs[@]}"; do
      IFS=',' read -r -a alts <<<"${spec}"
      pkg_install_any "${alts[@]}" || true
    done
  else
    echo "Skipping missing package install."
  fi
else
  echo "All listed packages are installed."
fi
if [[ "${PM}" == apt ]]; then
  if ask_yes "Update listed apt packages?" y "${CODUOS_APT_UPGRADE:-}"; then
    export DEBIAN_FRONTEND=noninteractive
    if [[ "${APT_UPDATED:-0}" -ne 1 ]]; then
      apt-get update -qq
      APT_UPDATED=1
    fi
    upgrade=()
    for spec in "${all_specs[@]}"; do
      IFS=',' read -r -a alts <<<"${spec}"
      for p in "${alts[@]}"; do
        p="${p#"${p%%[![:space:]]*}"}"
        [[ -z "${p}" ]] && continue
        if dpkg-query -W -f='${Status}' "${p}" 2>/dev/null | grep -q 'install ok installed'; then
          upgrade+=("${p}")
        fi
      done
    done
    if [[ ${#upgrade[@]} -gt 0 ]]; then
      apt-get install -y "${upgrade[@]}" || true
    fi
  fi
fi
ensure_docker_compose

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
