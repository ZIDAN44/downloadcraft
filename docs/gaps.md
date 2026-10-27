# Gaps

Missing features and known problems, most important first. Remove an item once it's done.

1. **Interface text through craft-i18n** (`tl!` / `tf!` and the catalogs, as MailCraft); every
   string is English in code today.
2. **Graphics setup and the log file** from MailCraft (`graphics.rs`, `logfile.rs`): the software
   renderer fallback on Windows and a message box when the window can't open.
3. **One instance per data folder** (`single_instance.rs`), so a second launch with a link hands
   it to the running window.
4. **SonarCloud and Nix** (`sonarcloud.yml`, `nix.yml`, `flake.nix`) as the storytold apps have
   them: SonarCloud needs the project and a `SONAR_TOKEN` secret set up by the owner.
5. **Fuzz targets** for `Content-Range`, `Content-Disposition` and `source::classify`.
6. **Video pages: the rest.** Playlists and channels (one link now downloads one video);
   subtitles; FFmpeg fetched on macOS and Linux (their package managers have it today); private or
   members-only videos, which would need the user's sign-in.
7. **Show in folder on Linux** was checked through its parts (the `file://` address, the D-Bus
    call), not yet against a running desktop's file manager.
