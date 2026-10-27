# Gaps

Missing features and known problems, most important first. Remove an item once it's done.

1. **Idle-read timeout.** ureq 3 has a timeout for the whole body, not per read, so a server that
   stops sending mid-file holds the transfer at 0 B/s until the user pauses (and a resume waits
   for that thread). Read through a socket with `set_read_timeout`, or wrap the reader.
2. **Interface text through craft-i18n** (`tl!` / `tf!` and the catalogs, as MailCraft); every
   string is English in code today.
3. **Graphics setup and the log file** from MailCraft (`graphics.rs`, `logfile.rs`): the software
   renderer fallback on Windows and a message box when the window can't open.
4. **One instance per data folder** (`single_instance.rs`), so a second launch with a link hands
   it to the running window.
5. **SonarCloud and Nix** (`sonarcloud.yml`, `nix.yml`, `flake.nix`) as the storytold apps have
   them: SonarCloud needs the project and a `SONAR_TOKEN` secret set up by the owner.
6. **Free-space check** before a start, and a clear message when the disk fills.
7. **Fuzz targets** for `Content-Range`, `Content-Disposition` and `source::classify`.
8. **A per-server memory of how many connections it allows.** A server that caps connections
   (answering 429 or 503, or not at all) is coped with per download, by handing parts back; some
   then slow the whole client down for a while, so learning the cap would help.
9. **Video pages: the rest.** Playlists and channels (one link now downloads one video);
   subtitles; FFmpeg fetched on macOS and Linux (their package managers have it today); private or
   members-only videos, which would need the user's sign-in.
10. **Show in folder on Linux** was checked through its parts (the `file://` address, the D-Bus
    call), not yet against a running desktop's file manager.
