#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
  echo "Run as root" >&2
  exit 1
fi

systemctl disable --now coduosd.service 2>/dev/null || true
rm -f /usr/bin/coduosd
rm -f /usr/lib/systemd/system/coduosd.service
rm -rf /usr/share/coduos
systemctl daemon-reload

if [[ "${1:-}" == "--purge" ]]; then
  rm -rf /etc/coduos /var/lib/coduos
  echo "CoduOS removed, including data."
else
  echo "CoduOS removed. Config and data kept in /etc/coduos and /var/lib/coduos (use --purge to delete)."
fi
