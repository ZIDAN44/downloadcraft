# Gaps

Missing features and known problems, most important first. Remove an item once it's done.

1. **More languages.** Right-to-left (Arabic, Hebrew) and CJK interfaces need fonts embedded (as
   craft-fonts gives the storytold apps) and bidirectional layout; Bengali and other Indic scripts
   need text shaping egui doesn't do yet. Moving the catalogs to craft-i18n once DownloadCraft is
   one of its apps.
2. **SonarCloud** (`sonarcloud.yml`, as the storytold apps have it): needs the project and a
   `SONAR_TOKEN` secret set up by the owner first, or the workflow only fails.
3. **Video pages: the rest.** FFmpeg on macOS (there is no build from yt-dlp's project; Homebrew
   has it). Private or members-only videos, which would need the user's sign-in
   (`--cookies-from-browser`).
