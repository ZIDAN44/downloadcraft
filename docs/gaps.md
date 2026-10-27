# Gaps

Missing features and known problems, most important first. Remove an item once it's done.

1. **Interface text through craft-i18n** (`tl!` / `tf!` and the catalogs, as MailCraft); every
   string is English in code today.
2. **SonarCloud and Nix** (`sonarcloud.yml`, `nix.yml`, `flake.nix`) as the storytold apps have
   them: SonarCloud needs the project and a `SONAR_TOKEN` secret set up by the owner.
3. **Fuzz targets** for `Content-Range`, `Content-Disposition` and `source::classify`.
4. **Video pages: the rest.** Playlists and channels (one link now downloads one video);
   subtitles; FFmpeg fetched on macOS and Linux (their package managers have it today); private or
   members-only videos, which would need the user's sign-in.
5. **Show in folder on Linux** was checked through its parts (the `file://` address, the D-Bus
    call), not yet against a running desktop's file manager.
