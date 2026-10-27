# Updates

How DownloadCraft keeps itself up to date on Windows, macOS and Linux. Nothing else updates it (it
isn't part of any launcher or store), so every installed copy does it itself. Ported from
MailCraft's updater. The code: `apps/downloadcraft/src/self_update.rs` (the updater) and
`self_update/install.rs` (the helper that puts the new version in place),
`crates/net/src/updates.rs` (GitHub's list and the resumable, checked download),
`crates/ui-egui/src/updates.rs` (what the window says).

Two channels:

- **Releases** (`release.yml`, signed) follow published releases only, never a pre-release.
- **Test builds** (`release-wip.yml`, which sets `DOWNLOADCRAFT_UPDATE_CHANNEL=test` at compile
  time) follow the test builds (pre-releases) as well as releases.

A developer's build never changes itself: it isn't run from an installed location the updater
recognises. `DOWNLOADCRAFT_NO_AUTO_UPDATE=1` turns updating off for a run.

## What happens

1. **Checks.** A minute after start, then hourly (±10 minutes) while DownloadCraft is open. GitHub
   is asked with the last `ETag`, so an unchanged list costs nothing against its anonymous quota
   of 60 an hour. A failed check is tried again after 5 minutes, then 10, 20, 40, then hourly.
2. **Download.** The installer of the kind this copy came from goes to `<data>/updates`, written to
   `<file>.<size>.part` as it comes; a download cut off carries on with a range request, in this
   run or the next.
3. **Checked.** The file is kept only if its SHA-256 is what GitHub records for it (`digest`), or
   else what the release's `SHA256SUMS.txt` says, and its size is the release's. A resumed file
   that doesn't match is fetched once more from the start.
4. **Said.** A bar: *DownloadCraft 0.2.0 is ready. Restart now to install it, or it installs when
   you close DownloadCraft.* with **Restart now** and **Later** (Later hides the bar; it still
   installs on closing). The status bar shows the download's progress.
5. **Installed.** A detached helper waits for DownloadCraft to end, puts the new version in place
   and, with Restart now, opens it. Downloads that were running are saved as queued, so they start
   again by themselves in the new version and carry on from their partial files. A marker keeps
   the version just installed from being offered again for a day.

| Installed from | The helper | Administrator? |
|---|---|---|
| `.msi` | `msiexec /i … /passive` | No: it installs for this user, by default in `%LOCALAPPDATA%\Programs\DownloadCraft` |
| portable `.zip` | unzipped over the app's folder; `DownloadCraftData` stays | No |
| `.dmg` | `DownloadCraft.app` copied out in place of this one | No, where this user may write (`/Applications` on an admin account, or `~/Applications`) |
| `.AppImage` | the new file renamed over this one | No, where this user may write |

`.deb`, `.rpm`, Flatpak, the tarballs and FreeBSD are updated by their package managers.

To try it against the newest test build, run a test build with
`DOWNLOADCRAFT_UPDATE_PRETEND_VERSION=0.0.1` (it counts as that version for this run) and
`DOWNLOADCRAFT_UPDATE_FIRST_CHECK=5` (seconds before the first check).

## Windows: per user, no administrator

The installer is a setup wizard with a folder page (default
`%LOCALAPPDATA%\Programs\DownloadCraft`, or another the user picks), a "Create a desktop shortcut"
box, and a Finish page that offers to launch it. The folder and the shortcut choice are recorded
under `HKCU\Software\Learning Machines LLC\DownloadCraft`; updates (run with `/passive`, no
wizard) land in the same folder, and the updater reads `InstallDir` there to know this copy came
from the MSI. `windows-arm64.yml` checks all of it on every push.

MailCraft's MSI installs into Program Files for everyone on the PC, so every update makes Windows
ask for an administrator. DownloadCraft's MSI installs for the user running it instead, the way VS
Code's user installer, Discord and Chrome's per-user install do. Nothing asks for an administrator,
neither the first install nor any update, so the updater installs quietly.

What that costs:
- **Each Windows account installs it separately.** Someone else on the same PC doesn't get it.
- **The scope is fixed.** A per-user and a per-machine install of one product don't upgrade each
  other, so moving to per-machine later would mean a new `UpgradeCode` and moving people over.
  DownloadCraft has no installs yet, so starting per-user costs nothing.
- **Some company PCs block apps in user folders** (AppLocker or WDAC rules that allow only Program
  Files). There an IT department installs software anyway; the portable zip or a per-machine
  build could be added for them later.
- SmartScreen and antivirus treat it the same either way: what matters there is code signing.

## Not done, and why

- **A signature on the update.** The SHA-256 comes from the same GitHub release as the file, so it
  proves the file arrived whole, not who made it. Signing updates with an Ed25519 key (as Tauri and
  Sparkle do) needs a key kept as a secret of the release workflow: the owner's to make.
- **Smaller downloads.** The whole installer is fetched each time.
