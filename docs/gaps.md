# Gaps

Missing features and known problems in v0.1, most important first. Remove an item once it's done.

1. **Idle-read timeout.** ureq 3 has a timeout for the whole body, not per read, so a server that
   stops sending mid-file holds the transfer at 0 B/s until the user pauses (and a resume waits
   for that thread). Read through a socket with `set_read_timeout`, or wrap the reader.
2. **Segmented downloads** (v0.2): N connections on N byte ranges into one preallocated file,
   with the ranges saved so a resume continues each.
3. **Interface text through craft-i18n** (`tl!` / `tf!` and the catalogs, as MailCraft); every
   string is English in code today.
4. **Graphics setup and the log file** from MailCraft (`graphics.rs`, `logfile.rs`): the software
   renderer fallback on Windows and a message box when the window can't open.
5. **One instance per data folder** (`single_instance.rs`), so a second launch with a link hands
   it to the running window.
6. **The craft-launcher catalog entry**, and SonarCloud and Nix (`sonarcloud.yml`, `nix.yml`,
   `flake.nix`) as the storytold apps have them: SonarCloud needs the project and a `SONAR_TOKEN`
   secret set up by the owner.
7. **Free-space check** before a start, and a clear message when the disk fills.
8. **Fuzz targets** for `Content-Range`, `Content-Disposition` and `source::classify`.
