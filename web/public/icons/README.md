# CoduOS icons

Drop files here (`web/public/icons/`) or in the runtime overlay (`data/icons/` locally,
`/var/lib/coduos/icons` on an installed NAS). Replacing a file with the same name
updates that icon. SVG, PNG, and WebP work.

Runtime overlay wins over bundled files and does not need a UI rebuild — refresh the browser.

| File name | Used for |
| --- | --- |
| `files.svg` | Files app |
| `apps.svg` | Apps |
| `services.svg` | Services app |
| `settings.svg` | Settings |
| `install.svg` | Install |
| `docker.svg` | Compose apps without their own icon |
| `{app-id}.svg` | App whose id is `{app-id}` (e.g. `jellyfin.svg`) |
| `folder.svg` | Directories |
| `folder-{name}.svg` | Folder named `{name}` (e.g. `folder-media.svg`) |
| `{ext}.svg` | Files with that extension (`mkv.svg`, `pdf.svg`) |
| `image.svg` `video.svg` `audio.svg` `text.svg` `archive.svg` | Generic type fallbacks |
| `cpu.svg` `memory.svg` `disk.svg` | Desktop widgets |

Bundled artwork is a subset of [Reversal](https://github.com/yeyushengfan258/Reversal-icon-theme) (GPL-3.0). See `COPYING`.
