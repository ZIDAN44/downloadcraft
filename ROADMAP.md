# Roadmap

**Stage:** v0.1, with v0.2's multi-part downloads, a theme setting and the redesigned window (2026-10-27). Each milestone below is a version,
named for what it adds. The ranked work list for the current milestone is
[docs/gaps.md](docs/gaps.md).

## Headline numbers

| | |
|---|---|
| Crates | 4 libraries + 2 apps + xtask |
| Commands | 18 (`downloadcraft-cli commands`) |
| Platforms | Windows, macOS, Linux |

## Milestones

1. **v0.1 — Direct downloads** ✅ HTTP(S) with resume, a queue, the server's file names, the
   saved list, categories, paste-to-add, light and dark, the CLI.
2. **v0.2 — Faster** ✅ (early) multi-part downloads on up to 16 connections, written in place and
   resumed part by part. Still to do: per-download and global speed limits; an idle-read
   timeout and automatic retries with back-off; checking free disk space before starting.
3. **v0.3 — Torrents** `.torrent` files and magnet links (BEP 3, 9, 10), DHT (BEP 5), web seeds
   (BEP 19), choosing files inside a torrent, seeding to a ratio. Permissive crates only (no
   libtorrent-rasterbar bindings, no GPL clients' code).
4. **v0.4 — More places** FTP and FTPS, sign-in for HTTP (kept in the system keychain), cookies
   handed over from the browser extension, scheduled start and stop, "shut down when done".
5. **v0.5 — Sites** Video pages ✅ (early) through yt-dlp, with a quality choice. Still to do: MEGA
   file and folder links through MEGA's published API (decryption on this computer, the key never
   leaves it); playlists. Never around DRM, paywalls or sign-in walls.
6. **v0.6 — Everywhere** A browser extension (Chrome, Firefox, Edge) that hands links and
   downloads to the app; the clipboard watched for links.
   (Installers and self-updating test builds landed early, in v0.1.)
7. **v0.7 — Polish** Fourteen languages through craft-i18n, right-to-left layouts, the list
   sortable and searchable, a details pane with the response headers, checksums (SHA-256)
   checked after download.
