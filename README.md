<h1 align="center">DownloadCraft</h1>

<p align="center">
  <b>An open-source, clean-room alternative to Internet Download Manager, written in Rust.</b>
</p>

<p align="center">
  Paste a link and watch it download on several connections at once. Pause it, close the app, and
  pick up tomorrow where it stopped. Files, video pages, torrents and magnet links work today; MEGA
  and FTP are on the <a href="ROADMAP.md">roadmap</a>. No ads, no bundled offers, no telemetry:
  DownloadCraft talks only to the servers you point it at.
</p>

<p align="center">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-1f6fd6?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on Windows, macOS and Linux" src="https://img.shields.io/badge/runs%20on-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-3a8aee?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-1f6fd6?style=flat-square">
  <img alt="Agent-drivable: commands and CLI" src="https://img.shields.io/badge/agents-commands%20%C2%B7%20CLI-3a8aee?style=flat-square">
</p>

<p align="center">
  <img src="docs/images/downloads.png" alt="DownloadCraft in dark mode: a sidebar with Add link, the categories with counts, the total speed, the download folder and Settings; a header with search and the theme switch; an Ubuntu image downloading at 39.6 MB/s on 8 connections, its bar showing the 8 parts filling in, then paused, failed and completed downloads" width="100%">
</p>

<table>
<tr>
<td width="50%"><img src="docs/images/add-link.png" alt="The Add link dialog with four links pasted, one per numbered line with a dot for each: two direct links in green, shown below with their file names; a magnet link and a YouTube page in amber; the folder to save to, Start downloading now, and Add 2 downloads" width="100%"><p align="center"><sub><b>Paste several links at once.</b> Each line is checked as you type: green downloads now, amber needs something first, red isn't a link.</sub></p></td>
<td width="50%"><img src="docs/images/settings.png" alt="Settings in the light theme: Theme with Dark, Light and System, the download folder, downloads at once and connections per download" width="100%"><p align="center"><sub><b>Light, dark or your system's theme</b>, and how many connections each download may open.</sub></p></td>
</tr>
</table>

## Features

### Downloads

- **Several connections at once.** A file is split into parts and fetched on up to 16
  connections (8 by default), each part written straight into place. A connection that finishes
  early takes over half of the biggest part left, so all of them stay busy to the end. On
  cdn.kernel.org, a 140 MB file took 1.8 s on 8 connections and 23 s on one.
- **Resume after anything.** Pausing, losing the connection or quitting keeps the partial file and
  where every part got to; Resume asks the server for only the missing bytes. A connection that
  stalls for 30 s, or a server that's briefly busy, is retried by itself.
- **Polite to busy servers.** When a server turns extra connections away, those connections hand
  their parts to the others, and later downloads from that server open fewer for a while.
- **The real file name, before you add it.** Each pasted link is looked at for its name (from the
  server, else the address after redirects) and size. Names are made safe for every file system;
  a taken name becomes `name (1).ext`.
- **Room checked first.** A download whose size is known checks the disk's free space before it
  starts.
- **A queue.** Up to 3 downloads run at once by default (1–16 in Settings); the rest wait.

### Video pages

- **YouTube, Vimeo, Dailymotion, Twitch, TikTok, X and more**, through
  [yt-dlp](https://github.com/yt-dlp/yt-dlp). Pick a quality per video, each shown with its size
  (1080p · 107 MB, 720p · 85 MB, Audio only). Within a resolution, H.264 video and AAC audio are
  preferred, so the MP4 plays nearly everywhere.
- **Playlists and channels**: the link lists its videos (up to 500), and all of them are added at
  one quality.
- **Subtitles** in the languages you set, inside the video.
- yt-dlp, FFmpeg (which joins video and sound) and Deno (which YouTube's pages need) aren't
  bundled. DownloadCraft fetches each from its official GitHub release when you ask, and keeps it
  only if it matches the published checksum.

Download only videos you have the right to. DRM-protected videos aren't supported.

### Torrents

- **Magnet links and `.torrent` files**: paste the link, open the file with DownloadCraft, drop it
  on the window, or pick it with **Open a torrent file** in Add link.
- **Pick the files you want.** Add link lists the files inside, all ticked; untick the ones you
  don't want, and only the rest appear in the folder.
- **Nothing written over.** Every path in the torrent is checked first. A file already in the
  folder is never replaced; a resume checks what's on disk and fetches only what's missing.
- **Sharing back.** A finished torrent is shared while DownloadCraft is open until it has sent its
  own size (0 to 10 times, in Settings).
- Peers are found through trackers and the DHT, with
  [librqbit](https://github.com/ikatson/rqbit). DownloadCraft opens no ports on your router.

Download and share only what you have the right to.

### The window

- **Paste anywhere.** Ctrl+V in the window opens Add link with what you copied, one link per line.
  Opening a link with DownloadCraft while it runs adds it to the window already open.
- **Categories** (All, Downloading, Paused, Completed, Failed) with counts, and **search** by name
  or link.
- **Show in folder** opens the file manager with the file selected, as browsers do. A file that's
  been deleted or moved shows as **Deleted**, with **Download again**.
- **Remove asks first**, with a box to delete the file too (it goes to the Recycle Bin or Trash).
- **Light, dark or your system's theme.**
- **Eight languages**: English, Deutsch, español, français, italiano, Nederlands, português do
  Brasil and русский. DownloadCraft follows your system's language unless you pick one in
  Settings.

### Under the hood

- **The list is kept** between runs (`downloads.json` in the data folder). A damaged file is set
  aside, never fatal. `downloadcraft.log` beside it records what each run did.
- **Built for scripts and agents.** Every action is a command with an id, shared by the buttons,
  the shortcuts, `--cmd` and the CLI (`downloadcraft-cli get <url>`, `downloadcraft-cli commands`).

## Installing

Test builds are on the [releases page](https://github.com/ZIDAN44/downloadcraft/releases):

| Platform | Files |
|---|---|
| Windows x64, x86, ARM64 | `.msi` (installs for you only; no administrator needed) or `-portable.zip` |
| macOS (Apple silicon and Intel) | `.dmg` |
| Linux x86_64, aarch64 | `.AppImage`, `.deb`, `.rpm`, `.flatpak`, `.tar.gz` |
| Linux riscv64, FreeBSD | `.deb`, `.rpm`, `.tar.gz` |
| Nix (Linux, Apple silicon) | `nix run github:ZIDAN44/downloadcraft`, or the flake's package and overlay |

Installed from the `.msi`, the portable zip, the `.dmg` or the `.AppImage`, DownloadCraft updates
itself. It downloads the next version in the background, checks its SHA-256, and installs it when
you restart. Copies from releases follow releases; copies from test builds also follow test
builds. [How updates work](docs/updates.md).

## Using it

| Keys | Does |
|---|---|
| Ctrl+N | Add a link |
| Ctrl+V | Paste a link (anywhere in the window) |
| Ctrl+F | Search |
| Space | Pause or resume the selected download |
| Delete | Remove the selected download |
| ↑ ↓ | Move through the list |
| Enter | Open a finished file |

Right-click a download for Resume, Pause, Open, Show in folder, Copy link and Remove.

From a terminal:

```bash
downloadcraft-cli get https://example.com/file.zip --dir ./downloads --connections 8
```

`--subtitles en,es` adds subtitles to videos, and `downloadcraft-cli tools get` fetches yt-dlp,
FFmpeg and Deno. `downloadcraft-cli files <torrent>` lists the files in a torrent or magnet link,
and `get <torrent> --files 1,3-5` downloads only those.

## Building from source

```bash
cargo run -p downloadcraft
```

Before a commit, run the same checks CI runs:

```bash
cargo xtask ci
```

DownloadCraft is Rust throughout: egui for the window, ureq over rustls for HTTP. A sans-I/O engine
(`crates/engine`) owns the list and the queue, the network layer (`crates/net`) runs each transfer
on its own thread, and the window (`crates/ui-egui`) only calls commands. Read
[the architecture](docs/architecture.md) and [the contributor guide](AGENTS.md) next.

## Licence

MIT OR Apache-2.0, at your choice. DownloadCraft is a clean-room project: it's built from the
published specifications and from public behaviour, never from another download manager's code,
and every asset is original (the icons are drawn in code). See [ATTRIBUTION.md](ATTRIBUTION.md).
