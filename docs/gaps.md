# Gaps

The work for the next milestone and the known gaps, most important first. When an item is done,
remove it and record it in the [Progress log](../ROADMAP.md#progress-log).

1. **More from torrents.**
   - Web seeds (BEP 19): librqbit has none, so a torrent served only from a web server (as many of
     archive.org's are) finds no one to download from.
   - A resume reads every file of the torrent again to check it; on a big torrent that takes a
     while. librqbit's fast resume needs its session state kept, which DownloadCraft doesn't keep
     yet. For the same reason a magnet link asks peers for its details again on every start.
   - Finished torrents stop sharing when DownloadCraft closes and don't start again on its next
     start.
2. **Speed limits**, per download and overall (librqbit has them for torrents).
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
