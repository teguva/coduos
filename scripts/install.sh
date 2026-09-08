#!/usr/bin/env bash
# Install CoduOS from a GitHub Release tarball (or a local tarball path).
# First-time only: prompts for Docker, nginx, and WireGuard.
# To update an existing install, use scripts/update.sh.
#
# Non-interactive (CI / unattended):
#   CODUOS_DOCKER=yes CODUOS_NGINX=yes CODUOS_WIREGUARD=no sudo -E bash scripts/install.sh
set -euo pipefail

REPO="${CODUOS_REPO:-teguva/coduos}"
VERSION="${CODUOS_VERSION:-latest}"
LOCAL_TARBALL="${1:-}"

if [[ ${EUID} -ne 0 ]]; then
  echo "Run as root: sudo bash scripts/install.sh" >&2
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
      echo "${prompt} -> yes (no TTY; override with CODUOS_DOCKER / CODUOS_NGINX / CODUOS_WIREGUARD)"
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

echo "CoduOS installer (first-time setup)"
echo "Choose which host services to install and enable."
echo
echo "Already installed? Update instead:"
echo "  curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/update.sh | sudo bash"
echo

if [[ -x /usr/bin/coduosd ]]; then
  echo "CoduOS is already on this machine (/usr/bin/coduosd)."
  echo "This script is the first-time installer (Docker / nginx / WireGuard)."
  echo "To update CoduOS only, run:"
  echo "  curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/update.sh | sudo bash"
  echo
  if [[ "${CODUOS_FORCE_INSTALL:-}" != "1" ]]; then
    echo "Refusing to re-run the installer. Set CODUOS_FORCE_INSTALL=1 to continue anyway." >&2
    exit 1
  fi
  echo "CODUOS_FORCE_INSTALL=1 set; continuing with first-time installer prompts."
  echo
fi

WITH_DOCKER=0
WITH_NGINX=0
WITH_WG=0
if ask_yes "Install and enable Docker? (needed for Apps)" y "${CODUOS_DOCKER:-}"; then
  WITH_DOCKER=1
fi
if ask_yes "Install and enable nginx? (dashboard on ports 80/443 and reverse proxy)" y "${CODUOS_NGINX:-}"; then
  WITH_NGINX=1
fi
if ask_yes "Install WireGuard tools and enable the CoduOS VPN unit?" n "${CODUOS_WIREGUARD:-}"; then
  WITH_WG=1
fi
echo

echo "Installing packages…"
base_pkgs=()
case "${PM}" in
  apt) base_pkgs+=(curl tar ca-certificates) ;;
  pacman) base_pkgs+=(curl tar ca-certificates) ;;
  dnf) base_pkgs+=(curl tar ca-certificates) ;;
  apk) base_pkgs+=(curl tar ca-certificates) ;;
esac
if [[ ${#base_pkgs[@]} -gt 0 ]]; then
  pkg_install "${base_pkgs[@]}" || true
fi
if ! command -v curl >/dev/null 2>&1 || ! command -v tar >/dev/null 2>&1; then
  echo "curl and tar are required" >&2
  exit 1
fi

if [[ "${WITH_DOCKER}" -eq 1 ]]; then
  case "${PM}" in
    apt) pkg_install_any docker.io docker-ce docker ;;
    pacman) pkg_install docker ;;
    dnf) pkg_install_any docker docker-ce moby-engine ;;
    apk) pkg_install docker ;;
    *) echo "Install Docker yourself, then re-run." >&2 ;;
  esac
fi
if [[ "${WITH_NGINX}" -eq 1 ]]; then
  pkg_install nginx
  case "${PM}" in
    apt) pkg_install openssl || true ;;
    *) pkg_install openssl || true ;;
  esac
fi
if [[ "${WITH_WG}" -eq 1 ]]; then
  pkg_install wireguard-tools
  case "${PM}" in
    apt) pkg_install iptables || true ;;
  esac
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "${tmpdir}"' EXIT

if [[ -n "${LOCAL_TARBALL}" ]]; then
  tar -xzf "${LOCAL_TARBALL}" -C "${tmpdir}"
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

install -d /usr/bin /usr/share/coduos /usr/lib/systemd/system /etc/coduos \
  /var/lib/coduos /var/lib/coduos/nginx/conf.d /var/lib/coduos/nginx/acme \
  /var/lib/coduos/wireguard /media/coduos
install -m 0755 "${root}/usr/bin/coduosd" /usr/bin/coduosd
rm -rf /usr/share/coduos/www
cp -a "${root}/usr/share/coduos/www" /usr/share/coduos/www
install -m 0644 "${root}/usr/lib/systemd/system/coduosd.service" /usr/lib/systemd/system/coduosd.service
if [[ -f "${root}/usr/lib/systemd/system/coduos-wg.service" ]]; then
  install -m 0644 "${root}/usr/lib/systemd/system/coduos-wg.service" /usr/lib/systemd/system/coduos-wg.service
fi
if [[ ! -f /etc/coduos/coduos.toml ]]; then
  install -m 0644 "${root}/etc/coduos/coduos.toml" /etc/coduos/coduos.toml
  if [[ -d /DATA ]]; then
    if ! grep -q 'id = "data"' /etc/coduos/coduos.toml; then
      cat >> /etc/coduos/coduos.toml <<'EOF'

[[file_roots]]
id = "data"
label = "Data"
path = "/DATA"
EOF
    fi
  fi
fi

systemctl daemon-reload

if [[ "${WITH_DOCKER}" -eq 1 ]]; then
  systemctl enable --now docker.service 2>/dev/null || systemctl enable --now docker 2>/dev/null || true
fi

if [[ "${WITH_NGINX}" -eq 1 ]]; then
  snippet="${root}/usr/share/coduos/nginx-coduos.conf"
  if [[ -f "${snippet}" ]]; then
    if [[ -d /etc/nginx/conf.d ]]; then
      install -m 0644 "${snippet}" /etc/nginx/conf.d/coduos.conf
    elif [[ -d /etc/nginx ]]; then
      install -d /etc/nginx/conf.d
      install -m 0644 "${snippet}" /etc/nginx/conf.d/coduos.conf
      if [[ -f /etc/nginx/nginx.conf ]] && ! grep -q 'conf.d/\*\.conf' /etc/nginx/nginx.conf; then
        echo "Add  include /etc/nginx/conf.d/*.conf;  to nginx.conf if nginx does not load conf.d." >&2
      fi
    fi
  fi
  if [[ -L /etc/nginx/sites-enabled/default ]]; then
    rm -f /etc/nginx/sites-enabled/default
  fi
  if [[ ! -f /var/lib/coduos/nginx/conf.d/00-dashboard.conf ]]; then
    cat >/var/lib/coduos/nginx/conf.d/00-dashboard.conf <<'EOF'
server {
  listen 80 default_server;
  listen [::]:80 default_server;
  server_name _;
  location / {
    proxy_pass http://127.0.0.1:13209;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
  }
}
EOF
  fi
fi

if [[ "${WITH_WG}" -eq 1 ]]; then
  systemctl enable coduos-wg.service 2>/dev/null || true
fi

systemctl enable --now coduosd.service

if [[ "${WITH_NGINX}" -eq 1 ]]; then
  if nginx -t 2>/dev/null; then
    systemctl enable --now nginx.service 2>/dev/null || systemctl enable --now nginx
    systemctl reload nginx.service 2>/dev/null || systemctl reload nginx || true
  else
    echo "nginx -t failed; CoduOS is on 127.0.0.1:13209. Fix nginx, then: systemctl reload nginx" >&2
  fi
fi

lan=$(hostname -I 2>/dev/null | awk '{print $1}')
echo
echo "CoduOS installed."
echo "  daemon:  http://127.0.0.1:13209/"
if [[ "${WITH_NGINX}" -eq 1 ]]; then
  echo "  nginx:   http://${lan:-<host>}/"
else
  echo "  nginx:   skipped"
fi
if [[ "${WITH_DOCKER}" -eq 1 ]]; then
  echo "  Docker:  enabled"
else
  echo "  Docker:  skipped (Apps will not run until Docker is installed)"
fi
if [[ "${WITH_WG}" -eq 1 ]]; then
  echo "  VPN:     WireGuard tools installed; turn on VPN in the CoduOS UI"
else
  echo "  VPN:     skipped"
fi
echo "Saying no does not uninstall packages already on the system."
