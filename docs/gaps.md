# Gaps

The work for the next milestone and the known gaps, most important first. When an item is done,
remove it and record it in the [Progress log](../ROADMAP.md#progress-log).

1. **More from torrents.**
   - FreeBSD has no torrents: `librqbit-dualstack-sockets` 0.7 (through librqbit) asks for a
     socket option FreeBSD hasn't got (`SO_BINDTODEVICE`) and doesn't build there, so the FreeBSD
     packages leave librqbit out and say so (`crates/net/src/torrent_off.rs`). The fix belongs in
     that crate: `bind_device` only where socket2 has it (Linux, Android, Fuchsia).
   - Web seeds and peers take turns rather than working at once: the web seeds go first, then
     peers fill in. The HTTP downloader has no speed limit yet, so a torrent under a download
     limit (its own or the overall one) leaves its web seeds unused.
   - A resume reads every file of the torrent again to check it; on a big torrent that takes a
     while. librqbit's fast resume needs its session state kept, which DownloadCraft doesn't keep
     yet. For the same reason a magnet link asks peers for its details again on every start.
   - Finished torrents stop sharing when DownloadCraft closes and don't start again on its next
     start.
   - Priorities beyond first and the rest (librqbit downloads a file or doesn't), a torrent's own
     speed limits changed while it runs (they apply from its next start), and a limit on
     connections over all torrents.
   - A SOCKS5 proxy with a password (it needs the system keychain, coming in v0.5), and the DHT
     and UDP trackers through a proxy (librqbit sends those directly).
   - Each peer's progress and country, and a tracker's next announce: librqbit doesn't report
     them, and DownloadCraft uses librqbit as published rather than a changed copy.
2. **Speed limits** for files, video pages and web seeds.
3. **More video pages.**
   - FFmpeg on macOS: yt-dlp's project publishes no build for it, so for now Homebrew's is
     suggested.
   - Private and members-only videos would need the user's sign-in
     (`--cookies-from-browser`), which DownloadCraft doesn't hand to yt-dlp.
4. **More languages.** Right-to-left (Arabic, Hebrew) and CJK interfaces need embedded fonts (as
   craft-fonts gives the storytold apps) and bidirectional layout; Bengali and other Indic scripts
   need text shaping that egui doesn't do yet. The catalogs move to craft-i18n once DownloadCraft
   is one of its apps.
5. **SonarCloud** (`sonarcloud.yml`, as the storytold apps have it). The owner needs to set up the
   project and a `SONAR_TOKEN` secret first; without them the workflow can only fail.
