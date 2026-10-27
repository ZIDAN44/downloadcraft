# Architecture

```
apps/downloadcraft  ──┬─> crates/ui-egui (L6) ──> crates/engine (L4) ──> crates/model (L0)
apps/downloadcraft-cli┴─> crates/net (L3) ────────────────────────────> crates/model (L0)
```

A crate depends only on crates in lower layers; `cargo xtask layers` checks it. The engine never
depends on the network layer: the apps wire the two together.

## The loop

1. The UI (or the CLI, or `--cmd`) runs a command: `session.run("download.add", {"url": …})`.
2. The command changes the list and the session's `pump` starts queued downloads while slots are
   free, queueing a `Job::Fetch` for each with a fresh **run number**.
3. The app hands `session.take_jobs()` to the `Worker`, which starts a thread per transfer.
4. The thread reports `Event::Started`, `Progress` (four times a second), then `Finished` or
   `Failed`; the app feeds them to `session.apply`.
5. `apply` drops any event whose run number isn't the download's current one. Pausing sets the
   run to 0 and queues `Job::Stop`, so a transfer that is still waking up when it's told to stop
   can't change the list.

## A transfer (`crates/net/src/http.rs`)

- The file downloads into `name.dcpart` in its folder. A resume sends `Range: bytes=<size>-`.
- `206 Partial Content` must start where the partial file ends (`Content-Range`); anything else
  fails rather than corrupting the file. `200 OK` replaces the partial file. `416` on a resume
  means the partial file is already whole.
- On the first start only, the server's `Content-Disposition` name replaces the one from the link,
  and a taken name becomes `name (1).ext`.
- Before writing, the bytes still to come are checked against the disk's free space (`fs4`); a
  disk that fills anyway (no size given) fails with "the disk is full", keeping the partial file.
- The whole file is renamed into place; a short file (connection closed early) stays partial.
- A read that waits 30 s for a byte drops the connection (`timeout_per_read`). A dropped or
  stalled connection, a timeout, 408, 429 or 5xx is tried again from the partial file after 2 s,
  doubling to at most 30 s; five tries in a row with no new bytes fail the download with "Resume
  carries on". Other answers (404, 403…) fail at once.
- A new run for a download waits for the previous run's thread to leave, so two threads never
  write one partial file. Removing a download deletes its partial file after the same wait.

## Several connections (`crates/net/src/http/segmented.rs`)

- A first start asks for `Range: bytes=0-`. A `206` with a size of at least 2 MiB means the file
  can come in parts: it is split into up to `Prefs::connections` parts (at least 1 MiB each) and
  the partial file is sized to the whole file. The first answer serves the first part; the other
  connections ask for `bytes=start-end` each.
- Each connection reserves the bytes it may write under a lock, then writes them at their offset.
  A connection that finishes takes over the back half of the biggest part still going (never
  closer than 1 MiB to where that part's connection is), so all of them stay busy to the end.
- A connection that fails while others still work gives its part back and ends (servers that cap
  connections answer 429 or 503, or leave the extra ones waiting: those wait 15 s). The last one
  tries three times before the download fails.
- Where every part is goes to `name.dcpart.parts` once a second and on stopping, so Resume carries
  on each part. The record is checked to cover the file exactly once before it is trusted.

## The saved list (`apps/downloadcraft/src/store.rs`)

`Session::save` → `downloads.json` (written beside and renamed into place) every couple of seconds
when something changed, and on quitting. On loading, downloads that were running come back paused.
