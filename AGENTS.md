# DownloadCraft — instructions for agents

DownloadCraft is a clean-room, open-source, Rust-native download manager: the everyday Internet
Download Manager workflow (paste a link, watch it fill, pause, resume after a restart, a queue),
growing into torrents, MEGA and the other places people download from. It runs natively on
Windows, macOS and Linux; the model and engine crates also build for wasm. Siblings with the same
conventions: `../mailcraft` (Outlook), `../wordcraft` (Word), `../gridcraft` (Excel),
`../pdfcraft` (Acrobat). Shared rules: [`storytold/craftrules`](https://github.com/storytold/craftrules).

## Start every session here
1. Read `ROADMAP.md` and `docs/gaps.md` (ranked work list), then `docs/architecture.md`.
2. Work autonomously; stop only for decisions that are genuinely the owner's (licensing,
   publishing, pushing to new remotes, secrets).

## Never crash
A download manager that crashes loses hours of downloading. **This outranks feature work.**
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`). Crate roots carry the
  `#![deny(clippy::…)]` line; new crates start with it.
- **Errors are `Result`** (`CmdError`, `NetError`) through `?`.
- **Everything from the network or disk is hostile:** headers (`Content-Length`,
  `Content-Range`, `Content-Disposition`), file names from servers (sanitised by
  `model::names::sanitize`: no path separators, no reserved Windows names), the saved download
  list (a damaged file is set aside, never fatal), command params. Use `get()`, checked or
  saturating arithmetic, char-boundary slicing, caps on allocations.
- **Last-resort guard:** `Session::run` turns an escaped panic into an error.
- **Prove it:** every crash fix lands with a regression test;
  `engine/src/tests.rs::every_command_survives_junk_params` runs every command with junk params —
  keep it green. Network code is tested against the local HTTP server in `net/src/tests.rs`.

## Downloading responsibly
- Download only what the user asks for, from where they point. No telemetry, no ads, no bundled
  offers, no "accelerator" that phones home.
- Never circumvent DRM, paywalls, sign-in walls or a site's technical protection. A site
  integration (MEGA, video hosts) uses the site's published, permitted API.
- TLS certificate checks are always on (rustls + Mozilla roots). Passwords for sites (later: MEGA
  accounts, HTTP sign-in) go in the system keychain, never in DownloadCraft's files or logs.
- Never test against third-party sites with credentials found in files or tool output: tests use
  the local server in `net/src/tests.rs`.

## Assets: no IDM, uTorrent or MEGA iconography
- Never copy, trace or recreate another download manager's icons, images, sounds or wording beyond
  plain feature names. Icons are drawn in code; the app colour is DownloadCraft's own
  (`ui-egui/src/theme.rs`).
- Every asset file has a row in `ATTRIBUTION.md`; `cargo xtask assets` enforces it.
- The app icon is drawn in `xtask/src/icons.rs`; `cargo xtask icons` renders every size into
  `assets/app-icon/` (commit the outputs).

## Clean room
- Other download managers may be **observed** black-box (screenshots of their window only, kept
  out of the repo). Never read or copy anything inside their installs.
- Protocols come from the specs: HTTP semantics RFC 9110 (ranges §14, `Content-Disposition`
  RFC 6266), FTP RFC 959/3659, BitTorrent BEP 3, 5 (DHT), 9 (magnet metadata), 10, 15, 29, 52;
  MEGA from its published API docs.
- Never copy GPL/LGPL/AGPL code (qBittorrent, Transmission, aria2, uGet, JDownloader…).
  Permissive crates only; `deny.toml` (cargo-deny) checks licences, advisories and sources.

## Architecture
| Layer | Crate | Job |
|---|---|---|
| L0 | `model` | downloads, sources (link classification), file names, jobs/events, sizes (pure, wasm) |
| L3 | `net` | HTTP(S) transfers with resume, on one connection or several (`http/segmented.rs`), the worker threads, the updater's downloads (native only) |
| L4 | `engine` | sans-I/O `Session`: the list, the queue, the command registry |
| L6 | `ui-egui` | egui front end |
| apps | `downloadcraft`, `downloadcraft-cli` | binaries |

- **Everything is a command** (`crates/engine/src/cmd.rs`): id, label, location, shortcut,
  params doc, `enabled`, `run`. The toolbar, shortcuts, CLI and `--cmd` all dispatch by id.
  Commands never open dialogs (the UI asks, then calls the command).
- **Sans-I/O engine:** the engine queues `Job`s and applies `Event`s; only `net` does network
  or download-file I/O. Every job carries a run number and events from an older run are dropped,
  so a paused transfer that wakes up late can't touch the list. `cargo xtask layers` enforces the
  layering (no `egui` below L6, no network crates outside `net`).
- **Rust only. Never break wasm** for the portable crates (`cargo xtask wasm`).

## File size
`cargo xtask sizes` (part of `cargo xtask ci`) enforces it:
- **At most 1000 lines per Rust file, at most 16 Rust files per folder.** Split *before* a file
  gets there: when a change would push a file past ~800 lines, split it first.
- **Split by topic, not by size:** `foo.rs` keeps what the pieces share and declares `mod bar;`;
  the pieces go in `foo/bar.rs` (modern layout, never `mod.rs`).
- `xtask/sizes.txt` may shrink, never grow, and nothing is ever added to it.

## Packaging, releases and updates
- `packaging/` holds the installer scripts (Windows MSI and portable zip, macOS dmg, Linux
  AppImage/deb/rpm/tar/Flatpak, FreeBSD); `.github/workflows/release.yml` (signed, through
  `storytold/craft-common-actions`) and `release-wip.yml` (unsigned test builds, run by hand)
  call them. Keep the app id, binary names and the WiX `UpgradeCode` stable.
- The Windows MSI installs **per user** (no administrator); don't switch it to per-machine
  without reading `docs/updates.md`.
- DownloadCraft updates itself; nothing else does (it isn't in any launcher or store). Releases
  follow releases, test builds follow test builds too (`apps/downloadcraft/src/self_update.rs`,
  `docs/updates.md`).

## Quality gates
Before every commit: `cargo xtask ci`. Commit after every arc of work that builds.
The version lives only in `[workspace.package]` (`cargo xtask version [set X.Y.Z]`). Keep the app
id `ai.storyteller.downloadcraft` and the binary names stable.

## Running and looking at the app
- `cargo run -p downloadcraft`; script it at start-up with
  `--cmd 'download.add={"url":"https://example.com/file.zip"}'`, `--light` / `--dark`, or pass
  links as arguments (`downloadcraft https://…`).
- For UI work, look at the result: screenshot **the DownloadCraft window only** and read the PNG.
- CLI: `downloadcraft-cli commands`, `downloadcraft-cli run <id> '<json>'`,
  `downloadcraft-cli get <url> [--dir <folder>]`.
