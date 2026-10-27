# Roadmap

**Stage:** v0.3 — steadier transfers, playlists and subtitles, eight languages (2026-10-27).

Each milestone below is a version, named for what it adds. The [Progress log](#progress-log)
records what changed in each, and [docs/gaps.md](docs/gaps.md) ranks the work for the next one.

**Versions.** The version lives only in `[workspace.package]` in `Cargo.toml`; change it with
`cargo xtask version set X.Y.Z`. Once a test build of a version is published (`v0.2.0-test.6`),
later work belongs to the next version: raise the version and add its Progress log entry before
the next build, so no two different builds share a version.

## Headline numbers

| | |
|---|---|
| Crates | 4 libraries, 2 apps and xtask |
| Commands | 20 (`downloadcraft-cli commands`) |
| Tests | 110: unit tests, a local HTTP server for downloads on one or many connections and for the updater, and headless frames of the window |
| Platforms | Windows, macOS, Linux (and FreeBSD packages) |

## Milestones

1. **v0.1 — Direct downloads** ✅ HTTP(S) with resume, a queue, the server's file names, the
   saved list, categories, paste-to-add, light and dark themes, and the CLI.
2. **v0.2 — Faster, and videos** ✅ Downloads on up to 16 connections, written in place and
   resumed part by part; video pages through yt-dlp with a choice of quality; the real file name
   and size while adding; Show in folder, deleted files marked Deleted, and Remove that can delete
   the file; a redesigned window with a theme switch; releases that update themselves.
3. **v0.3 — Steadier, and in your language** ✅ Retries with back-off after a stall or a busy
   server, a free-space check, a per-server connection limit; one window per data folder and a log
   file; playlists, channels and subtitles from video pages, with FFmpeg on Linux and Deno for
   YouTube; the window in eight languages; fuzz testing and a Nix flake.
4. **v0.4 — Torrents** `.torrent` files and magnet links (BEP 3, 9, 10), DHT (BEP 5), web seeds
   (BEP 19), choosing files inside a torrent, and seeding to a ratio, built on permissive crates
   only (no libtorrent-rasterbar bindings, no GPL clients' code). Speed limits per download and
   overall.
5. **v0.5 — More places** FTP and FTPS; sign-in for HTTP, kept in the system keychain; cookies
   handed over from the browser extension; scheduled start and stop; "shut down when done".
6. **v0.6 — Sites** MEGA file and folder links through MEGA's published API, decrypted on your
   computer so the key never leaves it. Never around DRM, paywalls or sign-in walls.
7. **v0.7 — Everywhere** A browser extension (Chrome, Firefox, Edge) that hands links and downloads
   to the app, and an optional watch on the clipboard for links. (Installers and self-updating
   builds came early, in v0.1.)
8. **v0.8 — Polish** Fourteen languages through craft-i18n, right-to-left layouts, a sortable list,
   a details pane with the server's response headers, and SHA-256 checks after download.

## Progress log

- **2026-10-27** — **v0.1**: the workspace (model, network, engine and egui crates, the desktop app,
  the CLI and xtask); HTTP(S) downloads that resume with byte ranges; a queue; the server's file
  names; the saved list; categories; paste-to-add; light and dark themes. 45 tests.
- **2026-10-27** — **v0.1**: packaging and release workflows from MailCraft, on the shared Craft
  pipelines: a Windows MSI and portable zip for x64, x86 and ARM64; a macOS dmg; Linux AppImage,
  deb, rpm, tarball and Flatpak; riscv64 and FreeBSD packages. The Windows installer is a setup
  wizard that installs per user, with no administrator prompt. Test builds update themselves on
  Windows, macOS and Linux. Published as `v0.1.0-test.4`.
- **2026-10-27** — **v0.2**: downloads on up to 16 connections. Connections that finish take over
  parts still going, connections a server turns away hand their parts back, and every part resumes
  from its own record. A 140 MB file took 1.8 s on 8 connections and 23 s on one. A redesigned
  window (sidebar, search, rows with actions, a bar showing each part) with a System, Light and Dark
  switch, and an Add link box with one numbered, checked link per line.
- **2026-10-27** — **v0.2**: video pages (YouTube, Vimeo, Dailymotion, Twitch, TikTok, X and more)
  through yt-dlp, fetched on request and checked against its published SHA-256 (FFmpeg too, on
  Windows). A quality picker shows each size; H.264 and AAC are preferred, saved as MP4 under the
  title. Pasted links show their real file name and size. Show in folder selects the file; a deleted
  or moved file shows as Deleted, with Download again; Remove asks first and can send the file to
  the Recycle Bin or Trash. Signed releases update themselves too. `downloadcraft-cli tools`. 77
  tests. Published as `v0.2.0-test.6`.
- **2026-10-27** — **v0.3**: steadier transfers.
  - A connection silent for 30 s is dropped. Dropped connections, timeouts and busy servers (408,
    429, 5xx) are retried from where the file got to, after 2 s, then twice as long each time up to
    30 s. Five tries without a byte between them fail the download; a missing or forbidden file
    fails at once.
  - A download of known size checks the disk's free space before writing ("not enough room on the
    disk: 135 MB needed, 21.0 MB free"). A disk that fills while writing says so and keeps the
    partial file.
  - A server that turns connections away is remembered for 30 minutes, and later downloads from it
    open only as many connections as it served.
- **2026-10-27** — **v0.3**: the app around the downloads.
  - One window per data folder (from MailCraft): starting DownloadCraft again brings the open window
    forward, and its Add link box takes any links it was started with.
  - `downloadcraft.log` in the data folder (`--log` echoes it), and MailCraft's graphics set-up:
    DirectX 12 on Windows, Windows' software renderer on a PC without a GPU, and a message box with
    the reason when the window can't open.
  - Fuzz testing (`fuzz/`, run weekly) of file names, links, byte ranges, parts records, the
    updater's lists, yt-dlp's answers and the saved list. It found two file-name bugs straight
    away, both fixed.
  - A Nix flake (from LightCraft's), evaluated in CI and built once in a container with its tests
    passing. Show in folder checked against real Linux file managers: Nautilus and Thunar open the
    folder with the file selected.
- **2026-10-27** — **v0.3**: more from video pages.
  - FFmpeg fetched and checked on Linux and 32-bit Windows too.
  - Playlist and channel links list their videos (up to 500; a channel's tabs taken in turn;
    private, deleted and live videos left out) and add each at one quality, in the window and in
    `downloadcraft-cli get`.
  - Subtitles in the languages set in Settings (`--subtitles` in the CLI), inside the video when
    FFmpeg is there.
  - Deno for YouTube: yt-dlp now needs a JavaScript runtime there and offers fewer formats without
    one (53 against 61 on a 4K video). Add link offers to fetch Deno's official build, checked
    against its published checksum.
- **2026-10-27** — **v0.3**: the window in eight languages (English, German, Spanish, French,
  Italian, Dutch, Brazilian Portuguese and Russian), following the system's language or the one
  picked in Settings. Tests check that every string drawn is in every catalog and none is stale.
  Settings wraps a long line beside its control and scrolls in a short window. 110 tests.
  Published as `v0.3.0-test.7`.
- **2026-10-27** — **v0.3.1**: Add link says torrents arrive in v0.4, FTP in v0.5 and MEGA in v0.6,
  in step with the milestones; the documentation rewritten.
