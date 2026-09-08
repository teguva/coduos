# CoduOS

Lightweight local server dashboard: one Rust daemon (`coduosd`) and a Svelte UI. Docker Compose apps, a jailed file browser, resource widgets, settings, and allowlisted systemd units.

Not a CasaOS/ZimaOS fork. No IceWhale services, no app-store ZIP, no message bus.

## Install (Debian / Ubuntu)

Needs `curl` and `tar`. Docker is required for apps.

```sh
curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/install.sh | sudo bash
```

Open `http://<host>/`, create the admin account, then install apps from Compose YAML.

Update: run the same installer again (keeps `/etc/coduos/coduos.toml` and `/var/lib/coduos`).

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/uninstall.sh | sudo bash
# sudo bash -s -- --purge   # also delete config and data
```

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
scripts/          # install, uninstall, pack-release
```

Config: `/etc/coduos/coduos.toml` (bind, data dir, file roots, unit allowlist).
State: `/var/lib/coduos/coduos.db` and `/var/lib/coduos/apps/<id>/compose.yml`.

Host units are **allowlisted**. The UI never sends a free-form systemd unit name.
