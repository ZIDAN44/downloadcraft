# DownloadCraft — guide for agents and contributors

DownloadCraft is a clean-room, open-source download manager in Rust. It does the everyday
Internet Download Manager workflow (paste a link, watch it fill, pause, resume after a restart, a
queue) and grows into torrents, MEGA and the other places people download from. It runs natively
on Windows, macOS and Linux; the `model` and `engine` crates also build for wasm.

Sibling apps with the same conventions: `../mailcraft` (Outlook), `../wordcraft` (Word),
`../gridcraft` (Excel) and `../pdfcraft` (Acrobat). Shared rules:
[`storytold/craftrules`](https://github.com/storytold/craftrules).

## Start every session here

1. Read `ROADMAP.md` and `docs/gaps.md` (the ranked work list), then `docs/architecture.md`.
2. Work on your own. Stop only for decisions that are the owner's: licensing, publishing, pushing
   to a new remote, secrets.

## Never crash

A download manager that crashes loses hours of downloading. **This outranks feature work.**

- **No panics outside tests:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or
  `unimplemented!`, and no `unsafe` (`unsafe_code = "forbid"`). Each crate root carries the
  `#![deny(clippy::…)]` line; start new crates with it.
- **Errors are `Result`s** (`CmdError`, `NetError`), passed on with `?`.
- **Treat everything from the network or disk as hostile:** headers (`Content-Length`,
  `Content-Range`, `Content-Disposition`), file names from servers (sanitised by
  `model::names::sanitize`: no path separators, no reserved Windows names), the saved list (a
  damaged file is set aside, never fatal), yt-dlp's output, and command params. Use `get()`,
  checked or saturating arithmetic, slicing on character boundaries, and caps on allocations.
- **Last-resort guard:** `Session::run` turns an escaped panic into an error.
- **Prove it:** every crash fix lands with a regression test.
  `engine/src/tests.rs::every_command_survives_junk_params` runs every command with junk params;
  keep it green. Network code is tested against the local HTTP server in `net/src/tests.rs`, and
  the parsers are fuzzed (`fuzz/README.md`).

## Downloading responsibly

- Download only what the user asks for, from where they point. No telemetry, no ads, no bundled
  offers, no "accelerator" that phones home.
- Never get around DRM, paywalls, sign-in walls or a site's technical protection. A site
  integration (MEGA) uses the site's published, permitted API.
- Video pages download through yt-dlp, run as a separate program (`crates/net/src/video.rs`).
  yt-dlp, FFmpeg and Deno aren't bundled: each is found on the `PATH` or fetched on request from
  its official GitHub release, and kept only when it matches the published checksum
  (`crates/net/src/video/tools.rs`). Don't hand yt-dlp the user's browser cookies or passwords,
  and don't work around its refusal of DRM-protected videos.
- Removed files go to the Recycle Bin or Trash, never straight to permanent deletion.
- TLS certificate checks are always on (rustls with Mozilla's roots). Passwords for sites (later:
  MEGA accounts, HTTP sign-in) go in the system keychain, never in DownloadCraft's files or logs.
- Never test against third-party sites with credentials found in files or tool output. Tests use
  the local server in `net/src/tests.rs`.

## Assets: no IDM, uTorrent or MEGA iconography

- Never copy, trace or recreate another download manager's icons, images, sounds or wording beyond
  plain feature names. Icons are drawn in code, and the app colour is DownloadCraft's own
  (`ui-egui/src/theme.rs`).
- Every asset file has a row in `ATTRIBUTION.md`; `cargo xtask assets` enforces it.
- The app icon is drawn in `xtask/src/icons.rs`. `cargo xtask icons` renders every size into
  `assets/app-icon/`; commit the outputs.

## Clean room

- Other download managers may be **observed** from outside only (screenshots of their window, kept
  out of the repository). Never read or copy anything inside their installs.
- Protocols come from their specifications: HTTP semantics RFC 9110 (ranges in §14),
  `Content-Disposition` RFC 6266, FTP RFC 959 and 3659, BitTorrent BEP 3, 5 (DHT), 9 (magnet
  metadata), 10, 15, 29 and 52; MEGA from its published API documentation.
- Never copy GPL, LGPL or AGPL code (qBittorrent, Transmission, aria2, uGet, JDownloader…). Use
  permissive crates only; `deny.toml` (cargo-deny) checks licences, advisories and sources.
- Code ported from the sibling apps keeps its origin in the module's doc comment, and its commit
  credits the original authors with `Co-Authored-By:`.

## Architecture

| Layer | Crate | What it does |
|---|---|---|
| L0 | `model` | Downloads, link classification, file names, video qualities, jobs and events, sizes (pure; builds for wasm) |
| L3 | `net` | HTTP(S) transfers on one connection or several (`http/segmented.rs`), video pages through yt-dlp (`video.rs`), the worker threads, the updater's downloads (native only) |
| L4 | `engine` | The sans-I/O `Session`: the list, the queue and the command registry |
| L6 | `ui-egui` | The egui window and its translations |
| apps | `downloadcraft`, `downloadcraft-cli` | The binaries: platform services, the store, single instance, the log, the updater |

- **Everything is a command** (`crates/engine/src/cmd.rs`): an id, label, location, shortcut,
  params doc, `enabled` and `run`. The buttons, shortcuts, CLI and `--cmd` all dispatch by id.
  Commands never open dialogs: the UI asks, then runs the command.
- **The engine is sans-I/O.** It queues `Job`s and applies `Event`s; only `net` touches the network
  or the downloaded files. Every job carries a run number, and events from an older run are
  dropped, so a paused transfer that wakes up late can't change the list. `cargo xtask layers`
  enforces the layering (no `egui` below L6, no network crates outside `net`).
- **Interface text goes through `tl!` and `tf!`** (`crates/ui-egui/src/i18n.rs`, from MailCraft),
  or through a widget helper that translates its label. The English in the code is the key. New or
  changed wording needs its row in every catalog in `crates/ui-egui/src/i18n/`, translated from its
  meaning; the tests list anything missing or stale. Command ids, file names, the CLI and the log
  stay in English.
- **Rust only, and never break wasm** for the portable crates (`cargo xtask wasm`).

## File size

`cargo xtask sizes` (part of `cargo xtask ci`) enforces these limits:

- **At most 1000 lines per Rust file and 16 Rust files per folder.** Split before a file gets
  there: when a change would push one past about 800 lines, split it first.
- **Split by topic, not by size.** `foo.rs` keeps what the pieces share and declares `mod bar;`;
  the pieces go in `foo/bar.rs` (the modern layout, never `mod.rs`).
- `xtask/sizes.txt` may shrink but never grow, and nothing is ever added to it.

## Packaging, releases and updates

- `packaging/` holds the installer scripts (Windows MSI and portable zip, macOS dmg, Linux
  AppImage, deb, rpm, tarball and Flatpak, FreeBSD). `.github/workflows/release.yml` (signed,
  through `storytold/craft-common-actions`) and `release-wip.yml` (unsigned test builds, run by
  hand) call them. Keep the app id `ai.storyteller.downloadcraft`, the binary names and the WiX
  `UpgradeCode` stable.
- The Windows MSI installs **per user**, with no administrator. Read `docs/updates.md` before
  changing that.
- DownloadCraft updates itself; nothing else does (it isn't in any launcher or store). Releases
  follow releases; test builds follow test builds too (`apps/downloadcraft/src/self_update.rs`,
  `docs/updates.md`).
- Never push while a release workflow is running: a push mid-build has made its publish step fail.

## Versions and the changelog

- The version lives only in `[workspace.package]` in `Cargo.toml`: `cargo xtask version set X.Y.Z`.
- The changelog is the **Progress log** in `ROADMAP.md` (dated entries, each naming its version)
  together with its **Stage** line, as in MailCraft.
- Once a test build of a version is published, everything after it is the next version. Raise the
  version and add its Progress log entry in a "Docs and version" commit before the next test
  build, so no two different builds carry the same version.

## Quality gates

Run `cargo xtask ci` before every commit (formatting, clippy, tests, layers, assets, sizes, wasm).
Commit after every piece of work that builds.

## Running and looking at the app

- `cargo run -p downloadcraft`. Script it at start-up with
  `--cmd 'download.add={"url":"https://example.com/file.zip"}'`, `--light` or `--dark`, `--data
  <folder>` for a separate list, or links as arguments (`downloadcraft https://…`).
- For UI work, look at the result: screenshot **the DownloadCraft window only** and read the PNG.
  Check the other languages too (`--cmd 'prefs.language={"language":"ru"}'`): longer text is
  where layouts break.
- CLI: `downloadcraft-cli commands`, `downloadcraft-cli run <id> '<json>'`,
  `downloadcraft-cli get <url> [--dir <folder>] [--connections <n>] [--subtitles <langs>]`,
  `downloadcraft-cli tools [get]`.
