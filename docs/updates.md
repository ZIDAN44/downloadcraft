# Updates

How DownloadCraft keeps itself up to date on Windows, macOS and Linux. It isn't part of any
launcher or store, so every installed copy updates itself. The updater is ported from MailCraft's.

| Code | What it does |
|---|---|
| `apps/downloadcraft/src/self_update.rs` | The updater: when to check, what to fetch, when to install |
| `apps/downloadcraft/src/self_update/install.rs` | The helper that puts the new version in place |
| `crates/net/src/updates.rs` | GitHub's release list, and the resumable, checked download |
| `crates/ui-egui/src/updates.rs` | What the window shows |

There are two channels:

- **Releases** (built by `release.yml`, signed) follow published releases only, never a
  pre-release.
- **Test builds** (built by `release-wip.yml`, which sets `DOWNLOADCRAFT_UPDATE_CHANNEL=test` at
  compile time) follow test builds (pre-releases) as well as releases.

A developer's build never changes itself, because it isn't run from an installed location the
updater recognises. `DOWNLOADCRAFT_NO_AUTO_UPDATE=1` turns updating off for one run.

## What happens

1. **Check.** A minute after start, then every hour (±10 minutes) while DownloadCraft is open. The
   request to GitHub carries the last `ETag`, so an unchanged list doesn't count against GitHub's
   anonymous limit of 60 requests an hour. A failed check is retried after 5 minutes, then 10, 20
   and 40, then hourly.
2. **Download.** The installer of the kind this copy came from is saved to `<data>/updates` as
   `<file>.<size>.part` while it arrives. A download that's cut off carries on with a range request,
   in this run or the next.
3. **Verify.** The file is kept only if its size is the release's and its SHA-256 matches what
   GitHub records for it (`digest`), or else the release's `SHA256SUMS.txt`. A resumed file that
   doesn't match is downloaded once more from the start.
4. **Tell.** A bar says *DownloadCraft X.Y.Z is ready. Restart now to install it, or it installs
   when you close DownloadCraft*, with **Restart now** and **Later**. Later hides the bar; the
   update still installs on closing. The status bar shows the download's progress meanwhile.
5. **Install.** A detached helper waits for DownloadCraft to close, puts the new version in place
   and, after Restart now, opens it. Downloads that were running are saved as queued, so they
   start again in the new version and carry on from their partial files. A marker stops the
   version just installed from being offered again for a day.

| Installed from | How the helper updates it | Administrator needed? |
|---|---|---|
| `.msi` | `msiexec /i … /passive` | No: it installs for this user, by default in `%LOCALAPPDATA%\Programs\DownloadCraft` |
| portable `.zip` | Unzips over the app's folder; `DownloadCraftData` is kept | No |
| `.dmg` | Copies `DownloadCraft.app` over this one | No, wherever this user can write (`/Applications` on an admin account, or `~/Applications`) |
| `.AppImage` | Renames the new file over this one | No, wherever this user can write |

Package managers update the `.deb`, `.rpm`, Flatpak, tarball and FreeBSD installs.

To try an update against the newest test build, run a test build with
`DOWNLOADCRAFT_UPDATE_PRETEND_VERSION=0.0.1` (it counts as that version for this run) and
`DOWNLOADCRAFT_UPDATE_FIRST_CHECK=5` (seconds before the first check).

## Windows: per user, no administrator

The installer is a setup wizard. It has a folder page (`%LOCALAPPDATA%\Programs\DownloadCraft` by
default, or a folder you pick), a "Create a desktop shortcut" box, and a Finish page that offers to
start DownloadCraft. The folder and the shortcut choice are recorded under
`HKCU\Software\Learning Machines LLC\DownloadCraft`. Updates run with `/passive` (no wizard)
and land in the same folder; the updater reads `InstallDir` there to know this copy came from the
MSI.
`windows-arm64.yml` tests all of this on every push.

MailCraft's MSI installs into Program Files for everyone on the PC, so Windows asks for an
administrator on every update. DownloadCraft's MSI installs for the user running it instead, as VS
Code's user installer, Discord and Chrome's per-user install do. Nothing asks for an administrator,
neither the first install nor any update, so updates install quietly.

The trade-offs:

- **Each Windows account installs it separately.** Someone else on the same PC doesn't get it.
- **The choice is fixed.** A per-user and a per-machine install of one product don't upgrade each
  other, so moving to per-machine later would need a new `UpgradeCode` and a way to move people
  over. DownloadCraft has no installs yet, so starting per-user costs nothing.
- **Some company PCs block apps in user folders** (AppLocker or WDAC rules that allow only Program
  Files). An IT department installs software there anyway; the portable zip, or a per-machine build
  added later, would serve them.
- SmartScreen and antivirus treat both the same: what matters to them is code signing.

## Not done yet, and why

- **A signature on the update.** The SHA-256 comes from the same GitHub release as the file, so it
  proves the file arrived intact, not who made it. Signing updates with an Ed25519 key (as Tauri and
  Sparkle do) needs a key stored as a secret of the release workflow, which only the owner can
  create.
- **Smaller downloads.** The whole installer is downloaded every time.
