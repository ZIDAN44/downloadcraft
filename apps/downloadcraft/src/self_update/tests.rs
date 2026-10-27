use super::install::*;
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("downloadcraft-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn verbatim_paths_are_made_plain() {
    assert_eq!(plain(PathBuf::from(r"\\?\C:\Tools\DownloadCraft\downloadcraft.exe")), PathBuf::from(r"C:\Tools\DownloadCraft\downloadcraft.exe"));
    assert_eq!(plain(PathBuf::from(r"\\?\UNC\server\share\m.exe")), PathBuf::from(r"\\server\share\m.exe"));
    assert_eq!(plain(PathBuf::from("/Applications/DownloadCraft.app")), PathBuf::from("/Applications/DownloadCraft.app"));
}

#[test]
fn checks_spread_around_the_hour() {
    let waits: Vec<u64> = [0, 1, 599, 600, 1200, u64::MAX].iter().map(|s| next_wait(*s).as_secs()).collect();
    assert!(waits.iter().all(|w| (50 * 60..=70 * 60).contains(w)), "{waits:?}");
    assert_ne!(waits[0], waits[2]);
}

/// A check that fails (no connection, GitHub's quota used up) is tried again sooner than the
/// hour, then less and less often.
#[test]
fn failed_checks_back_off_up_to_the_hour() {
    let waits: Vec<u64> = (1..=8).map(|n| retry_wait(n).as_secs() / 60).collect();
    assert_eq!(waits, [5, 10, 20, 40, 60, 60, 60, 60]);
    assert_eq!(retry_wait(u32::MAX), EVERY);
}

#[test]
fn the_version_just_installed_isnt_offered_again_for_a_day() {
    let dir = scratch("marker");
    assert!(!just_installed(&dir, "v0.21.0-test.19", 1_000_000));
    std::fs::write(dir.join(MARKER), "v0.21.0-test.19\n1000000\n").unwrap();
    assert!(just_installed(&dir, "v0.21.0-test.19", 1_000_000 + 3600));
    assert!(!just_installed(&dir, "v0.21.0-test.20", 1_000_000 + 3600), "another version is offered");
    assert!(!just_installed(&dir, "v0.21.0-test.19", 1_000_000 + JUST_INSTALLED_FOR), "a day later it may be again");
    assert!(!just_installed(&dir, "v0.21.0-test.19", 999_000), "a clock gone back proves nothing");
    std::fs::write(dir.join(MARKER), "junk").unwrap();
    assert!(!just_installed(&dir, "junk", 1_000_000));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download left part way is carried on soon after start.
#[test]
fn a_partial_download_is_noticed() {
    let dir = scratch("part");
    assert!(!has_part(&dir) && !has_part(&dir.join("missing")));
    std::fs::write(dir.join("downloadcraft-0.25.0-windows-x64.msi"), b"").unwrap();
    assert!(!has_part(&dir));
    std::fs::write(dir.join("downloadcraft-0.25.0-windows-x64.msi.42.part"), b"").unwrap();
    assert!(has_part(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Paths with quotes and spaces stay one word in the helper's scripts.
#[test]
fn helper_scripts_quote_their_paths() {
    let odd = Path::new(r"C:\Users\O'Brien\My Apps\downloadcraft-0.21.0-windows-x64.msi");
    let msi = msi_step(odd);
    assert!(msi.contains(r#"'"C:\Users\O''Brien\My Apps\downloadcraft-0.21.0-windows-x64.msi"'"#), "{msi}");
    let zip = portable_step(Path::new(r"C:\Tools\DownloadCraft\x.zip"), Path::new(r"C:\Tools\Mail'Craft"));
    assert!(zip.contains(r"-Destination 'C:\Tools\Mail''Craft'"), "{zip}");
    // PowerShell ends a quoted string at a typographic quote as well: doubled too.
    assert_eq!(ps_quote("D:\\Zoë’s ‘mail’ ‚x‛"), "'D:\\Zoë’’s ‘‘mail’’ ‚‚x‛‛'");
    assert_eq!(sh_quote("/Applications/Mail'Craft.app"), r"'/Applications/Mail'\''Craft.app'");
    let mac = mac_step(Path::new("/tmp/m.dmg"), Path::new("/Applications/DownloadCraft.app"));
    assert!(mac.contains("ditto \"$m/DownloadCraft.app\" '/Applications/DownloadCraft.app'.new"), "{mac}");
    // Only a whole copy takes the app's place, and a failed move puts the old one back.
    assert!(mac.starts_with("rm -rf '/Applications/DownloadCraft.app'.new;"), "a copy left by an earlier try goes first");
    assert!(
        mac.contains("[ $ok -eq 0 ] &&") && mac.contains("|| mv '/Applications/DownloadCraft.app'.old '/Applications/DownloadCraft.app'"),
        "{mac}"
    );
}

/// The Mac step run by a shell, `hdiutil` and `ditto` stood in for: a disk image refused twice
/// (the system still busy with it) is attached the third time and the app replaced; one refused
/// every time leaves the app as it was.
#[cfg(unix)]
#[test]
fn a_busy_disk_image_is_attached_again() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("busy-dmg");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tries = dir.join("tries");
    let tool = |name: &str, body: &str| {
        let p = bin.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    // attach: refused while `tries` has no more than $REFUSE lines, then "mounts" the new app.
    let t = tries.display();
    tool(
        "hdiutil",
        &format!(
            "[ \"$1\" = attach ] || exit 0; echo x >> '{t}'; [ $(wc -l < '{t}') -gt $REFUSE ] || exit 1; mkdir -p \"$5/DownloadCraft.app\" && echo new > \"$5/DownloadCraft.app/v\""
        ),
    );
    tool("ditto", "cp -R \"$1\" \"$2\"");
    // The waits are cut short.
    tool("sleep", "exit 0");
    for (refuse, want) in [("2", "new"), ("99", "old")] {
        let _ = std::fs::remove_file(&tries);
        let app = dir.join("DownloadCraft.app");
        let _ = std::fs::remove_dir_all(&app);
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("v"), "old").unwrap();
        let dmg = dir.join("m.dmg");
        std::fs::write(&dmg, "").unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
        std::process::Command::new("sh").arg("-c").arg(mac_step(&dmg, &app)).env("PATH", path).env("REFUSE", refuse).status().unwrap();
        assert_eq!(std::fs::read_to_string(app.join("v")).unwrap().trim(), want, "refused {refuse} times");
        let attempts = std::fs::read_to_string(&tries).unwrap().lines().count();
        assert_eq!(attempts, if refuse == "2" { 3 } else { 7 }, "refused {refuse} times");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The kind of copy is told from where the exe is (Windows: beside a portable marker, or in
/// %LOCALAPPDATA%\Programs\DownloadCraft); the names pick the matching file of a release.
#[test]
fn the_install_kind_follows_where_downloadcraft_is() {
    if cfg!(windows) {
        let dir = scratch("kind");
        let app = dir.join("Programs").join("DownloadCraft");
        std::fs::create_dir_all(&app).unwrap();
        let exe = app.join("downloadcraft.exe");
        let local = dir.clone();
        let env = move |k: &str| (k == "LOCALAPPDATA").then(|| local.clone());
        assert_eq!(detect_at(&exe, &env), Some(Kind::Msi(exe.clone())));
        std::fs::write(app.join("portable.txt"), "").unwrap();
        assert_eq!(detect_at(&exe, &env), Some(Kind::Portable(exe.clone())), "the marker wins");
        let elsewhere = dir.join("Downloads").join("downloadcraft.exe");
        assert_eq!(detect_at(&elsewhere, &env), None, "a copy run from elsewhere isn't replaced");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Kind::Msi(exe.clone()).suffix().ends_with(".msi") && Kind::Portable(exe).suffix().ends_with("-portable.zip"));
    }
    assert_eq!(Kind::MacApp(PathBuf::from("/Applications/DownloadCraft.app")).suffix(), "-macos-universal.dmg");
    assert!(Kind::AppImage(PathBuf::from("/x")).suffix().ends_with(".AppImage"));
    // An AppImage is replaced only where this user may write beside it (/proc: nobody may, not
    // even root, as in CI's containers).
    if cfg!(target_os = "linux") {
        let dir = scratch("appimage");
        let image = dir.join("DownloadCraft-x86_64.AppImage");
        std::fs::write(&image, b"").unwrap();
        let found = image.clone();
        let env = move |k: &str| (k == "APPIMAGE").then(|| found.clone());
        assert_eq!(detect_at(Path::new("/tmp/.mount_x/usr/bin/downloadcraft"), &env), Some(Kind::AppImage(image)));
        let env = |k: &str| (k == "APPIMAGE").then(|| PathBuf::from("/proc/version"));
        assert_eq!(detect_at(Path::new("/tmp/.mount_x/usr/bin/downloadcraft"), &env), None, "a folder it can't write");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The window's view of it, and that closing with nothing ready starts nothing.
#[test]
fn the_window_sees_where_the_update_is() {
    let dir = scratch("state");
    let u = Updater {
        kind: Kind::Msi(PathBuf::from(r"C:\Users\me\AppData\Local\Programs\DownloadCraft\downloadcraft.exe")),
        data: dir.clone(),
        shared: Arc::default(),
    };
    assert_eq!(u.state(), UpdateState::None);
    assert!(!u.restart_after_close(), "nothing to restart into");
    // Each change wakes the window, which an idle or minimized one needs to see it.
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = woken.clone();
    u.set_waker(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    u.set_status(Status::Downloading { tag: "v0.25.0-test.22".into() });
    assert_eq!(woken.load(Ordering::SeqCst), 1);
    u.shared.progress.done.store(10, Ordering::Relaxed);
    u.shared.progress.total.store(40, Ordering::Relaxed);
    assert_eq!(u.state(), UpdateState::Downloading { version: "0.25.0-test.22".into(), done: 10, total: 40 });
    u.set_status(Status::Ready { tag: "v0.25.0-test.22".into(), file: dir.join("x.msi") });
    assert_eq!(u.state(), UpdateState::Ready { version: "0.25.0-test.22".into() });
    assert!(u.installs_on_close());
    // Closing with nothing ready: nothing is started.
    let idle = Updater { kind: Kind::Portable(PathBuf::from("m.exe")), data: dir.clone(), shared: Arc::default() };
    idle.closing();
    assert!(!idle.shared.started.load(Ordering::SeqCst) && idle.shared.stop.load(Ordering::SeqCst));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder chosen in the setup wizard is found through the one the MSI records.
#[test]
fn the_msi_folder_comes_from_the_registry_too() {
    let out = "\r\nHKEY_CURRENT_USER\\Software\\Learning Machines LLC\\DownloadCraft\r\n    InstallDir    REG_SZ    D:\\Apps\\DC\\\r\n\r\n";
    assert_eq!(parse_reg_value(out, "InstallDir").as_deref(), Some(r"D:\Apps\DC"));
    assert_eq!(parse_reg_value("    Other    REG_SZ    x", "InstallDir"), None);
    if cfg!(windows) {
        let dir = scratch("chosen");
        let exe = dir.join("downloadcraft.exe");
        let recorded = dir.clone();
        let env = move |k: &str| (k == INSTALL_DIR).then(|| recorded.clone());
        assert_eq!(detect_at(&exe, &env), Some(Kind::Msi(exe.clone())));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A signed (stable) build only ever moves to a published release; a test build takes test
/// builds too.
#[test]
fn stable_builds_skip_pre_releases() {
    let rel = |tag: &str, prerelease: bool| Release { tag: tag.into(), prerelease, ..Default::default() };
    let list = vec![rel("v0.3.0-test.4", true), rel("v0.2.0", false)];
    let tags = |l: Vec<Release>| l.into_iter().map(|r| r.tag).collect::<Vec<_>>();
    assert_eq!(tags(for_channel(list.clone(), false)), ["v0.2.0"]);
    assert_eq!(tags(for_channel(list, true)), ["v0.3.0-test.4", "v0.2.0"]);
}
