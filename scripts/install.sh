#!/usr/bin/env bash
# Install CoduOS from a GitHub Release tarball (or a local tarball path).
# First-time only: prompts for Docker, nginx, and WireGuard.
# To update an existing install, use scripts/update.sh.
#
# Non-interactive (CI / unattended):
#   CODUOS_DOCKER=yes CODUOS_NGINX=yes CODUOS_WIREGUARD=no \
#   CODUOS_DATA=/DATA CODUOS_DATA_SETUP=folder \
#   sudo -E bash scripts/install.sh
# DATA_SETUP: folder (mkdir on the system disk), mount (CODUOS_DATA_DEVICE=/dev/sdX1), skip
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
      echo "${prompt} -> yes (no TTY; override with CODUOS_DOCKER / CODUOS_NGINX / CODUOS_WIREGUARD / CODUOS_DATA_SETUP)"
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

ask_line() {
  local prompt="$1"
  local default="$2"
  local preset="${3:-}"
  local reply=""
  if [[ -n "${preset}" ]]; then
    printf '%s\n' "${preset}"
    return
  fi
  if [[ ! -r /dev/tty ]]; then
    printf '%s\n' "${default}"
    return
  fi
  read -r -p "${prompt} [${default}] " reply </dev/tty || true
  printf '%s\n' "${reply:-${default}}"
}

normalize_abs_path() {
  local p="$1"
  p="${p#"${p%%[![:space:]]*}"}"
  p="${p%"${p##*[![:space:]]}"}"
  [[ "${p}" == /* ]] || return 1
  [[ "${p}" == *..* ]] && return 1
  while [[ "${p}" == */ && "${p}" != / ]]; do
    p="${p%/}"
  done
  [[ -n "${p}" ]] || return 1
  printf '%s' "${p}"
}

data_path_blocked() {
  case "$1" in
    /|/bin|/boot|/boot/*|/dev|/dev/*|/etc|/etc/*|/home|/home/*|/opt|/proc|/proc/*|/root|/run|/run/*|/sys|/sys/*|/tmp|/tmp/*|/usr|/usr/*|/var|/var/log|/var/lib|/var/lib/coduos|/var/lib/coduos/*)
      return 0
      ;;
  esac
  return 1
}

toml_escape() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '%s' "${s}"
}

is_system_blockdev() {
  local dev="$1"
  local src
  for src in $(findmnt -n -o SOURCE / /boot /boot/efi /usr /home 2>/dev/null || true); do
    [[ "${src}" == "${dev}" ]] && return 0
  done
  return 1
}

data_fs_ok() {
  case "${1,,}" in
    ext2|ext3|ext4|xfs|btrfs|f2fs|exfat|ntfs|ntfs3) return 0 ;;
    *) return 1 ;;
  esac
}

lsblk_pair() {
  local line="$1"
  local key="$2"
  local rest="${line#*"${key}=\""}"
  if [[ "${rest}" == "${line}" ]]; then
    printf ''
    return
  fi
  printf '%s' "${rest%%\"*}"
}

# Prints: path<TAB>size<TAB>fstype<TAB>uuid<TAB>label
list_data_candidates() {
  local line path size fstype uuid label mp type
  while IFS= read -r line; do
    [[ -n "${line}" ]] || continue
    path=$(lsblk_pair "${line}" PATH)
    size=$(lsblk_pair "${line}" SIZE)
    fstype=$(lsblk_pair "${line}" FSTYPE)
    uuid=$(lsblk_pair "${line}" UUID)
    label=$(lsblk_pair "${line}" LABEL)
    mp=$(lsblk_pair "${line}" MOUNTPOINT)
    type=$(lsblk_pair "${line}" TYPE)
    [[ -n "${path}" && -n "${uuid}" && -z "${mp}" ]] || continue
    [[ "${type}" == part || "${type}" == disk || "${type}" == lvm ]] || continue
    if [[ "${type}" == disk ]] && lsblk -nr -o TYPE "${path}" 2>/dev/null | grep -qx part; then
      continue
    fi
    data_fs_ok "${fstype}" || continue
    is_system_blockdev "${path}" && continue
    printf '%s\t%s\t%s\t%s\t%s\n' "${path}" "${size}" "${fstype}" "${uuid}" "${label:-}"
  done < <(lsblk -P -o PATH,SIZE,FSTYPE,UUID,LABEL,MOUNTPOINT,TYPE 2>/dev/null || true)
}

pick_data_device() {
  local preset="${1:-}"
  local -a rows=()
  local i line path size fstype uuid label
  if [[ -n "${preset}" ]]; then
    if [[ ! -b "${preset}" ]]; then
      echo "Device ${preset} is not a block device." >&2
      return 1
    fi
    DATA_DEVICE="${preset}"
    DATA_UUID=$(lsblk -nr -o UUID "${preset}" | head -1)
    DATA_FSTYPE=$(lsblk -nr -o FSTYPE "${preset}" | head -1)
    DATA_LABEL=$(lsblk -nr -o LABEL "${preset}" | head -1)
    if [[ -z "${DATA_UUID}" ]] || ! data_fs_ok "${DATA_FSTYPE}"; then
      echo "${preset} needs a Linux/exFAT/NTFS filesystem and a UUID. Format it in Storage after install, or pick another disk." >&2
      return 1
    fi
    return 0
  fi
  if [[ ! -r /dev/tty ]]; then
    echo "CODUOS_DATA_SETUP=mount needs CODUOS_DATA_DEVICE=/dev/sdX1 when there is no TTY." >&2
    return 1
  fi
  while IFS= read -r line; do
    [[ -n "${line}" ]] && rows+=("${line}")
  done < <(list_data_candidates)
  if [[ ${#rows[@]} -eq 0 ]]; then
    echo "No unused formatted disks or partitions found."
    echo "Create ${DATA_PATH} as a folder for now. You can format and mount a disk in Storage later."
    return 1
  fi
  echo "Unused disks/partitions:"
  i=1
  for line in "${rows[@]}"; do
    IFS=$'\t' read -r path size fstype uuid label <<<"${line}"
    printf '  %d) %s  %s  %s' "${i}" "${path}" "${size}" "${fstype}"
    [[ -n "${label}" ]] && printf '  (%s)' "${label}"
    printf '\n'
    i=$((i + 1))
  done
  local reply=""
  if [[ -r /dev/tty ]]; then
    read -r -p "Mount which disk? [1] " reply </dev/tty || true
  fi
  reply="${reply:-1}"
  if [[ ! "${reply}" =~ ^[0-9]+$ ]] || (( reply < 1 || reply > ${#rows[@]} )); then
    echo "Invalid choice." >&2
    return 1
  fi
  IFS=$'\t' read -r DATA_DEVICE size DATA_FSTYPE DATA_UUID DATA_LABEL <<<"${rows[$((reply - 1))]}"
}

mount_data_device() {
  local dest="$1"
  install -d "${dest}"
  local opts="nosuid,nodev,noexec"
  case "${DATA_FSTYPE,,}" in
    vfat|fat|fat32|exfat|ntfs|ntfs3|msdos)
      opts="${opts},uid=0,gid=0"
      ;;
  esac
  if ! mount -o "${opts}" "UUID=${DATA_UUID}" "${dest}"; then
    echo "Could not mount UUID=${DATA_UUID} at ${dest}." >&2
    return 1
  fi
  echo "Mounted ${DATA_DEVICE} at ${dest}"
}

write_data_config() {
  local cfg="$1"
  local path_esc uuid_esc dev_esc label_esc
  path_esc=$(toml_escape "${DATA_PATH}")
  if ! grep -qE '^apps_dir[[:space:]]*=' "${cfg}"; then
    printf '\napps_dir = "%s/AppData"\n' "${path_esc}" >>"${cfg}"
  fi
  if ! grep -qE 'id = "data"' "${cfg}"; then
    cat >>"${cfg}" <<EOF

[[file_roots]]
id = "data"
label = "Data"
path = "${path_esc}"
EOF
  fi
  if [[ -n "${DATA_UUID:-}" ]]; then
    uuid_esc=$(toml_escape "${DATA_UUID}")
    dev_esc=$(toml_escape "${DATA_DEVICE:-}")
    label_esc=$(toml_escape "${DATA_LABEL:-}")
    if ! grep -qF "uuid = \"${uuid_esc}\"" "${cfg}"; then
      cat >>"${cfg}" <<EOF

[[storage_mounts]]
uuid = "${uuid_esc}"
mountpoint = "${path_esc}"
device = "${dev_esc}"
label = "${label_esc}"
EOF
    fi
  fi
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

DATA_PATH="/DATA"
DATA_SETUP="skip"
DATA_DEVICE=""
DATA_UUID=""
DATA_FSTYPE=""
DATA_LABEL=""
NEED_DATA=0
if [[ ! -f /etc/coduos/coduos.toml ]]; then
  NEED_DATA=1
elif ! grep -qE '^id = "data"$' /etc/coduos/coduos.toml; then
  NEED_DATA=1
fi
if [[ "${NEED_DATA}" -eq 1 ]]; then
  echo "Apps and media go under a Data folder (CasaOS-style AppData), usually /DATA on an extra disk."
  local_data=$(ask_line "Data folder path" "/DATA" "${CODUOS_DATA:-}")
  if ! DATA_PATH=$(normalize_abs_path "${local_data}"); then
    echo "Invalid path '${local_data}'. Using /DATA." >&2
    DATA_PATH="/DATA"
  fi
  if data_path_blocked "${DATA_PATH}"; then
    echo "${DATA_PATH} is a system path and cannot be the Data folder. Using /DATA." >&2
    DATA_PATH="/DATA"
  fi
  if [[ -n "${CODUOS_DATA_SETUP:-}" ]]; then
    case "${CODUOS_DATA_SETUP,,}" in
      folder|mkdir|dir) DATA_SETUP="folder" ;;
      mount|disk) DATA_SETUP="mount" ;;
      skip|none|no) DATA_SETUP="skip" ;;
      existing|use) DATA_SETUP="existing" ;;
      *) DATA_SETUP="folder" ;;
    esac
  elif mnt_at=$(findmnt -n -o TARGET "${DATA_PATH}" 2>/dev/null || true) && [[ "${mnt_at}" == "${DATA_PATH}" ]]; then
    echo "${DATA_PATH} is already mounted; CoduOS will use it for Files and AppData."
    DATA_SETUP="existing"
    DATA_DEVICE=$(findmnt -n -o SOURCE "${DATA_PATH}" 2>/dev/null | head -1 || true)
    DATA_UUID=$(findmnt -n -o UUID "${DATA_PATH}" 2>/dev/null | head -1 || true)
    DATA_FSTYPE=$(findmnt -n -o FSTYPE "${DATA_PATH}" 2>/dev/null | head -1 || true)
    DATA_LABEL=$(lsblk -nr -o LABEL "${DATA_DEVICE}" 2>/dev/null | head -1 || true)
  elif [[ ! -r /dev/tty ]]; then
    if [[ -n "${CODUOS_DATA:-}" ]]; then
      echo "No TTY; creating ${DATA_PATH} as a folder (set CODUOS_DATA_SETUP=mount and CODUOS_DATA_DEVICE to use a disk)."
      DATA_SETUP="folder"
    else
      echo "No TTY; skipping Data folder (set CODUOS_DATA=/DATA and CODUOS_DATA_SETUP=folder or mount)."
      DATA_SETUP="skip"
    fi
  else
    echo
    echo "How should ${DATA_PATH} be created?"
    echo "  1) Folder on the system disk"
    echo "  2) Mount an extra disk or partition here (keeps apps and photos off the system disk)"
    echo "  3) Skip — apps stay in /var/lib/coduos/apps"
    data_choice=""
    if [[ -r /dev/tty ]]; then
      read -r -p "Choose [2] " data_choice </dev/tty || true
    fi
    data_choice="${data_choice:-2}"
    case "${data_choice}" in
      1) DATA_SETUP="folder" ;;
      3) DATA_SETUP="skip" ;;
      *) DATA_SETUP="mount" ;;
    esac
  fi
  if [[ "${DATA_SETUP}" == mount ]]; then
    if [[ -d "${DATA_PATH}" ]] && [[ -n "$(ls -A "${DATA_PATH}" 2>/dev/null || true)" ]]; then
      echo "Files already in ${DATA_PATH} stay on the system disk and are hidden while a disk is mounted there. You do not need to delete them."
      if ! ask_yes "Mount a disk at ${DATA_PATH} anyway?" y "${CODUOS_DATA_MOUNT_OVER:-}"; then
        DATA_SETUP="folder"
      fi
    fi
  fi
  if [[ "${DATA_SETUP}" == mount ]]; then
    if ! pick_data_device "${CODUOS_DATA_DEVICE:-}"; then
      DATA_SETUP="folder"
    fi
  fi
  if [[ "${DATA_SETUP}" == existing && -z "${DATA_UUID}" ]]; then
    DATA_SETUP="folder"
  fi
  echo
fi
unset local_data data_choice

echo "Installing packages…"
base_pkgs=()
case "${PM}" in
  apt) base_pkgs+=(curl tar ca-certificates parted e2fsprogs openssl) ;;
  pacman) base_pkgs+=(curl tar ca-certificates parted e2fsprogs openssl) ;;
  dnf) base_pkgs+=(curl tar ca-certificates parted e2fsprogs openssl) ;;
  apk) base_pkgs+=(curl tar ca-certificates parted e2fsprogs openssl) ;;
esac
if [[ ${#base_pkgs[@]} -gt 0 ]]; then
  pkg_install "${base_pkgs[@]}" || true
fi
if ! command -v curl >/dev/null 2>&1 || ! command -v tar >/dev/null 2>&1; then
  echo "curl and tar are required" >&2
  exit 1
fi
if ! command -v parted >/dev/null 2>&1; then
  echo "warning: parted is not installed; whole-disk format in Storage will not work" >&2
fi
if ! command -v mkfs.ext4 >/dev/null 2>&1; then
  echo "warning: mkfs.ext4 is not installed; formatting needs e2fsprogs" >&2
fi
if ! command -v openssl >/dev/null 2>&1; then
  echo "warning: openssl is not installed; HTTPS and LAN CA need the openssl package" >&2
fi

if [[ "${WITH_DOCKER}" -eq 1 ]]; then
  case "${PM}" in
    apt)
      pkg_install_any docker.io docker-ce docker
      pkg_install_any docker-compose-v2 docker-compose-plugin
      ;;
    pacman)
      pkg_install docker
      pkg_install_any docker-compose docker-compose-plugin
      ;;
    dnf)
      pkg_install_any docker docker-ce moby-engine
      pkg_install_any docker-compose-plugin docker-compose
      ;;
    apk)
      pkg_install docker
      pkg_install_any docker-cli-compose docker-compose
      ;;
    *) echo "Install Docker yourself, then re-run." >&2 ;;
  esac
  if ! docker compose version >/dev/null 2>&1; then
    echo "warning: Docker Compose v2 is not installed; Apps need the docker-compose-v2 (or docker-compose-plugin) package" >&2
  fi
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
fi
if [[ "${DATA_SETUP}" != skip ]]; then
  if [[ "${DATA_SETUP}" == mount ]]; then
    mount_data_device "${DATA_PATH}" || {
      DATA_SETUP="folder"
      DATA_UUID=""
      DATA_DEVICE=""
      DATA_LABEL=""
    }
  fi
  if [[ "${DATA_SETUP}" == folder || "${DATA_SETUP}" == existing || "${DATA_SETUP}" == mount ]]; then
    install -d "${DATA_PATH}/AppData"
    write_data_config /etc/coduos/coduos.toml
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
if [[ "${NEED_DATA:-0}" -eq 1 ]]; then
  if [[ "${DATA_SETUP}" != skip ]]; then
    echo "  Data:    ${DATA_PATH} (${DATA_SETUP}) · apps in ${DATA_PATH}/AppData"
  else
    echo "  Data:    skipped (apps in /var/lib/coduos/apps)"
  fi
fi
echo "Saying no does not uninstall packages already on the system."
