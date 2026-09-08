# CoduOS

Lightweight local server dashboard: one Rust daemon (`coduosd`) and a Svelte UI. Docker Compose apps, a jailed file browser, resource widgets, settings, and allowlisted systemd units.

Not a CasaOS/ZimaOS fork. No IceWhale services, no app-store ZIP, no message bus.

## Install

Needs a supported package manager (`apt`, `pacman`, `dnf`, or `apk`). The installer asks **yes/no** for Docker (Apps), nginx (ports 80/443 and reverse proxy), and WireGuard (VPN), then installs those packages and enables the matching units.

```sh
curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/install.sh | sudo bash
```

Unattended:

```sh
sudo CODUOS_DOCKER=yes CODUOS_NGINX=yes CODUOS_WIREGUARD=no bash -c \
  'curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/install.sh | bash'
```

Open `http://<host>/` if nginx was enabled, otherwise `http://<host>:13209/`. Create the admin account, then install apps from Compose YAML.

## Update

Replaces the daemon and web UI. Keeps `/etc/coduos` and `/var/lib/coduos`. Does not touch Docker, nginx, or WireGuard.

```sh
curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/update.sh | sudo bash
```

On an installed NAS you can also use **Settings → Updates → Update now**.

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/uninstall.sh | sudo bash
# sudo bash -s -- --purge   # also delete config and data
```

Uninstall removes CoduOS files and our nginx snippet. It does **not** uninstall Docker, nginx, or WireGuard.

## Develop

```sh
# terminal 1
cargo run -p coduosd
# terminal 2
cd web && npm install && npm run dev
```

Unprivileged dev binds `http://127.0.0.1:13209` and stores data in `./data`. Vite on `:5173` proxies `/api`.

## Layout

```
crates/coduosd/   # Axum daemon
web/              # Svelte 5 SPA
packaging/        # systemd unit, default config, example compose
scripts/          # install, update, uninstall, pack-release
```

Config: `/etc/coduos/coduos.toml` (bind, data dir, file roots, unit allowlist).
State: `/var/lib/coduos/coduos.db` and `/var/lib/coduos/apps/<id>/compose.yml`.

Host units are **allowlisted**. The UI never sends a free-form systemd unit name.
