# Roadmap

**Stage:** v0.2 — downloads in parts, video pages, a redesigned window (2026-10-27). Each milestone
below is a version, named for what it adds; what changed in each is in the [Progress log](#progress-log).
The ranked work list for the current milestone is [docs/gaps.md](docs/gaps.md).

Versions: the version lives only in `[workspace.package]` (`cargo xtask version set X.Y.Z`). Once a
test build of a version has been published (`v0.1.0-test.4`), what's done after it belongs to the
next version: the version is raised and the Progress log gains its entry before the next build.

## Headline numbers

| | |
|---|---|
| Crates | 4 libraries + 2 apps + xtask |
| Commands | 18 (`downloadcraft-cli commands`) |
| Tests | 77 (unit, a local HTTP server for single and multi-part downloads and the updater's download, headless UI frames) |
| Platforms | Windows, macOS, Linux |

## Milestones

1. **v0.1 — Direct downloads** ✅ HTTP(S) with resume, a queue, the server's file names, the
   saved list, categories, paste-to-add, light and dark, the CLI.
2. **v0.2 — Faster, and videos** ✅ multi-part downloads on up to 16 connections, written in place
   and resumed part by part; video pages through yt-dlp with a quality choice; the real file name
   and size while adding; Show in folder, deleted files shown as Deleted, and Remove that can
   delete the file; a redesigned window with a theme switch; releases updating themselves.
3. **v0.3 — Torrents, and steadier transfers** `.torrent` files and magnet links (BEP 3, 9, 10),
   DHT (BEP 5), web seeds (BEP 19), choosing files inside a torrent, seeding to a ratio, with
   permissive crates only (no libtorrent-rasterbar bindings, no GPL clients' code). Also left from
   v0.2: per-download and global speed limits, an idle-read timeout and retries with back-off, and
   checking free disk space before starting.
4. **v0.4 — More places** FTP and FTPS, sign-in for HTTP (kept in the system keychain), cookies
   handed over from the browser extension, scheduled start and stop, "shut down when done".
5. **v0.5 — Sites** MEGA file and folder links through MEGA's published API (decryption on this
   computer, the key never leaves it); video playlists and subtitles (single videos landed early,
   in v0.2). Never around DRM, paywalls or sign-in walls.
6. **v0.6 — Everywhere** A browser extension (Chrome, Firefox, Edge) that hands links and
   downloads to the app; the clipboard watched for links.
   (Installers and self-updating test builds landed early, in v0.1.)
7. **v0.7 — Polish** Fourteen languages through craft-i18n, right-to-left layouts, the list
   sortable and searchable, a details pane with the response headers, checksums (SHA-256)
   checked after download.

## Progress log

- **2026-10-27** — **v0.1**: workspace (model, network, engine and egui crates, the desktop app and
  CLI, xtask), HTTP(S) downloads that resume with byte ranges, a queue, the server's file names, the
  saved list, categories, paste-to-add, light and dark; 45 tests.
- **2026-10-27** — **v0.1**: packaging and release workflows ported from MailCraft (Windows MSI and
  portable zip for x64, x86 and ARM64, macOS dmg, Linux AppImage/deb/rpm/tar/Flatpak, riscv64,
  FreeBSD) on the shared Craft pipelines; a per-user Windows installer with a setup wizard and no
  administrator prompt; test builds updating themselves on Windows, macOS and Linux. Published as
  `v0.1.0-test.4`.
- **2026-10-27** — **v0.2**: multi-part downloads (up to 16 connections, parts taken over as
  connections finish, connections a server turns away handing their part back, each part resumed
  from its record): a 140 MB file in 1.8 s on 8 connections against 23 s on one. A redesigned window
  (sidebar, search, rows with actions and a bar showing each part) and a System/Light/Dark theme
  switch; the Add link box with one numbered, checked link per line.
- **2026-10-27** — **v0.2**: video pages (YouTube, Vimeo, Dailymotion, Twitch, TikTok, X…) through
  yt-dlp, fetched on request and checked against its published SHA-256 (FFmpeg too, on Windows),
  with a quality picker showing each size, H.264 and AAC preferred, saved as MP4 under the title.
  Links looked at as they're pasted for their real file name and size. Show in folder with the
  file selected; a deleted or moved file shown as Deleted with Download again; Remove asking first,
  with a box to send the file to the Recycle Bin or Trash. Signed releases update themselves too;
  `downloadcraft-cli tools`. 77 tests, on Windows and Linux.
- **2026-10-27** — **v0.3**: a connection that sends nothing for 30 s is dropped, and a dropped
  connection, a timeout or a busy server (408, 429, 5xx) is tried again by itself from where the
  file got to, waiting 2 s, then twice as long each time up to 30 s; five tries without a byte in
  between fail the download, a missing or forbidden file fails at once.
- **2026-10-27** — **v0.3**: a download whose size is known checks the disk's free space before it
  writes anything ("not enough room on the disk: 135 MB needed, 21.0 MB free"), and a disk that
  fills while writing says so and keeps the partial file for Resume.
