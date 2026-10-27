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
- The whole file is renamed into place; a short file (connection closed early) stays partial and
  the download fails with "Resume carries on".
- A new run for a download waits for the previous run's thread to leave, so two threads never
  write one partial file. Removing a download deletes its partial file after the same wait.

## The saved list (`apps/downloadcraft/src/store.rs`)

`Session::save` → `downloads.json` (written beside and renamed into place) every couple of seconds
when something changed, and on quitting. On loading, downloads that were running come back paused.
