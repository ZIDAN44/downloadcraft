# Gaps

The work for the next milestone and the known gaps, most important first. When an item is done,
remove it and record it in the [Progress log](../ROADMAP.md#progress-log).

1. **Torrents.** `.torrent` files and magnet links, DHT, web seeds, choosing files inside a torrent,
   and seeding to a ratio (the [v0.4 milestone](../ROADMAP.md#milestones)). Such links are already
   recognised, and Add link says which version brings them.
2. **Speed limits**, per download and overall.
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
