# Architecture

```
apps/downloadcraft  ──┬─> crates/ui-egui (L6) ──> crates/engine (L4) ──> crates/model (L0)
apps/downloadcraft-cli┴─> crates/net (L3) ────────────────────────────> crates/model (L0)
```

A crate depends only on crates in lower layers, and `cargo xtask layers` checks it. The engine
never depends on the network layer: the apps connect the two.

## The loop

1. The window, the CLI or `--cmd` runs a command, such as
   `session.run("download.add", {"url": …})`.
2. The command changes the list. The session then starts queued downloads while slots are free,
   queueing a job for each (`Job::Fetch` for a file, `Job::FetchVideo` for a video page,
   `Job::FetchTorrent` for a torrent) with a new **run number**.
3. The app hands `session.take_jobs()` to the `Worker`, which runs each transfer on its own thread.
4. The thread reports `Event::Started`, then `Progress` about four times a second, then `Finished`
   or `Failed`. The app passes each to `session.apply`.
5. `apply` drops any event whose run number isn't the download's current one. Pausing sets the run
   to 0 and queues `Job::Stop`, so a transfer that is still starting when it's told to stop can't
   change the list.

## A transfer (`crates/net/src/http.rs`)

- The file downloads into `name.dcpart` in its folder. A resume sends `Range: bytes=<size>-`.
- A `206 Partial Content` answer must start where the partial file ends (`Content-Range`);
  anything else fails rather than corrupt the file. `200 OK` replaces the partial file, and `416`
  on a resume means the partial file is already complete.
- On the first start only, the server's `Content-Disposition` name replaces the one from the link,
  and a name that's taken becomes `name (1).ext`.
- Before writing, the bytes still to come are checked against the disk's free space (`fs4`). A disk
  that fills anyway (when the server gave no size) fails with "the disk is full" and keeps the
  partial file.
- A complete file is renamed into place. A short one (the connection closed early) stays partial.
- **Retries.** A read that waits 30 s for a byte drops the connection (`timeout_per_read`). A
  dropped or stalled connection, a timeout, or a 408, 429 or 5xx answer is retried from the partial
  file after 2 s, doubling to at most 30 s. Five tries in a row without new bytes fail the
  download, saying Resume carries on. Other answers (404, 403…) fail at once.
- A new run for a download waits for the previous run's thread to finish, so two threads never
  write one partial file. Removing a download deletes its partial file after the same wait.

## Several connections (`crates/net/src/http/segmented.rs`)

- A first start asks for `Range: bytes=0-`. A `206` answer for a file of at least 2 MiB means the
  file can come in parts. It's split into up to `Prefs::connections` parts of at least 1 MiB each,
  and the partial file is sized to the whole file. The first answer serves the first part; each
  other connection asks for its own `bytes=start-end`.
- Each connection reserves the bytes it may write under a lock, then writes them at their offset.
  A connection that finishes takes over the back half of the biggest part still going (never
  closer than 1 MiB to where that part's connection is), so all of them stay busy to the end.
- **Servers that limit connections.** A connection that fails while others still work hands its
  part back and ends. (Such servers answer 429 or 503, or leave the extra connections waiting;
  those give up after 15 s.) The last connection tries three times before the download fails. A
  connection turned away before its first byte marks the server (`host:port`) as allowing only the
  connections still going, and for 30 minutes later downloads from it open no more than that
  (`CAPS`).
- Where every part has got to is saved to `name.dcpart.parts` once a second and on stopping, so
  Resume carries on each part. The record is trusted only if it covers the file exactly once.

## Video pages (`crates/net/src/video.rs`, `crates/model/src/probe.rs`)

- A link is looked at with `yt-dlp -J --no-playlist --flat-playlist`. A video gives its title and
  qualities. A playlist or channel gives its videos (a channel's tabs one level down), and each is
  added as its own download at the quality picked for the whole list. A `watch?v=…&list=…` link
  means the one video.
- A download runs yt-dlp with the quality's format selector, `-S res,vcodec:h264,acodec:m4a` and MP4
  output. When Settings names subtitle languages, it adds `--write-subs --sub-langs <langs>`, plus
  `--embed-subs` when FFmpeg is available.
- yt-dlp, FFmpeg and Deno come from the `PATH` or the tools folder (`video/tools.rs`). Fetched
  copies come from their GitHub releases and are kept only when they match the published SHA-256:
  FFmpeg as a zip on Windows and a `.tar.xz` on Linux; Deno with a checksum file per asset, in
  PowerShell's format on Windows. yt-dlp gets `--js-runtimes deno:<path>` for a fetched Deno,
  which YouTube's pages need.

## Torrents (`crates/net/src/torrent.rs`, `torrent/storage.rs`)

- One librqbit session serves every torrent, on a Tokio runtime of its own (two threads) started
  with the first torrent from the torrent settings (`torrent/engine.rs`, given by
  `Job::Configure` when the list loads and whenever `prefs.torrent` changes). Speed limits change
  at once. A change to the port, protocol, UPnP, peer limit, proxy or IP filter starts a new
  session for the next torrent once none is running; torrents still going keep theirs. The DHT
  keeps its nodes in `<data>/torrents/dht.json`; an IP filter is written there as
  `ipfilter.p2p` (`.dat` lines converted) for librqbit to read.
- A transfer runs on its own thread, like an HTTP one. It asks librqbit for the torrent's details
  ("list only": for a magnet link, from peers, for up to 10 minutes; a `.torrent` link is fetched
  over HTTP(S), at most 16 MiB), checks every path in it with `names::unsafe_part`, and adds it with
  the files picked.
- A torrent of one file is written as `folder/name`; one of several, or one inside folders, as
  `folder/name/…`. On a first start (`first`: the download has no size yet) that name must not
  exist; later starts let librqbit read what's there and check it piece by piece.
- **Storage.** `PickedFactory` gives each torrent a storage of DownloadCraft's own. Picked files are
  opened when a piece of them arrives and given their full size once the torrent starts. Bytes
  that belong to files left out stay in memory (at most 512 MiB), enough to check and share the
  pieces they're in; after a restart those few pieces are fetched again. BEP 47 padding files are
  never stored.
- **Details.** Once a second the transfer sends `Event::Details` (`torrent/live.rs`): counts and
  speeds, up to 200 peers (each one's speed worked out from its bytes since the last look), the
  trackers and each file's progress. The first one carries the file list, which the engine keeps
  in the download, so files can be picked while it's paused. `Job::PickFiles` reaches the running
  transfer over a channel: the storage writes out what it kept aside for files newly picked, and
  librqbit is told the new choice.
- **Around librqbit.** DownloadCraft uses librqbit as published and does around it what it
  lacks. Web seeds (`torrent/web.rs`): before librqbit starts, each picked file that isn't on disk
  is fetched from the torrent's `url-list` (BEP 19 addresses) by `http::fetch`, into `name.dcpart`
  and renamed into place when whole, so a file at its path is complete; librqbit is then let
  write over them (`overwrite`) and checks them as it adds the torrent. When all came from the
  web and no peer appears for 20 s to check pieces shared with files left out, the transfer
  reports Finished itself. Download first: librqbit is given only those files, and the rest once
  it reports them done. A torrent's own limits go in `AddTorrentOptions::ratelimits`.
- **Scrape** (`torrent/scrape.rs`): a thread per torrent asks each tracker (at most 30) about the
  swarm, 3 s after the start and then every 15 minutes, over HTTP with a 64 KiB answer at most or
  over UDP, 12 s each. The answers are read in `model::torrent` (a bencode reader capped at 32
  levels and 100 000 items). With a proxy set, nothing is asked.
- Progress is `Event::Progress` twice a second. When it's all there, `Finished` is sent, and the
  thread keeps the torrent shared until it has sent `seed_ratio` times its size or is stopped,
  reporting `Event::Uploaded`. A finished torrent keeps its run number while it seeds, so only its
  own reports count; removing it (or Clear completed) queues `Job::Stop`.
- Removing an unfinished torrent that has started writing sends its files to the Recycle Bin or
  Trash (they're written in place, so there's no `.dcpart`). One that never started leaves the
  folder as it was.

## Start-up (`apps/downloadcraft/src/graphics.rs`, `logfile.rs`)

Both come from MailCraft.

- **The log.** `downloadcraft.log` in the data folder, new each run (the previous one is kept as
  `downloadcraft.old.log`), at most 4 MiB: DownloadCraft's own lines from Info up, and other crates'
  warnings.
- **Graphics.** On Windows the window is drawn with DirectX 12 only: on a real GPU when there is
  one, else with Windows' software renderer, WARP (`WGPU_FORCE_FALLBACK_ADAPTER=1` forces it). The
  device asks for what the adapter supports. After the first frames, a GPU error is logged rather
  than ending the app.
- **A window that can't open** shows a message box with the reason and the log's path. It frees
  the single-instance socket first.

## One window per data folder (`apps/downloadcraft/src/single_instance.rs`)

From MailCraft, which had it from PdfCraft. The first launch listens on a Unix domain socket
(`uds_windows` on Windows) in a folder only the user can reach, named after the data folder and
the screen. A later launch on the same data folder sends it one keyed JSON line with its links and
exits before it reads or writes the list, so two windows never overwrite `downloads.json`. The
links join the Add link box: a new one, the one already open, or one opened when another dialog
closes.

## The saved list (`apps/downloadcraft/src/store.rs`)

`Session::save` writes `downloads.json` (beside the old one, then renamed into place) every couple
of seconds when something changed, and on quitting. When the list is loaded, downloads that were
running come back paused.
