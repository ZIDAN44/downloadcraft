use std::sync::atomic::AtomicBool;

use super::*;
use crate::tests::{Serve, serve};

const LISTING: &str = r#"[
  {"tag_name": "v0.2.0-test.7", "draft": false, "target_commitish": "0123456789abcdef0123456789abcdef01234567",
   "assets": [
     {"name": "downloadcraft-0.2.0-windows-x64.msi", "size": 1234,
      "browser_download_url": "https://github.com/ZIDAN44/downloadcraft/releases/download/v0.2.0-test.7/downloadcraft-0.2.0-windows-x64.msi",
      "digest": "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"},
     {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/ZIDAN44/downloadcraft/releases/download/v0.2.0-test.7/SHA256SUMS.txt"}
   ]},
  {"tag_name": "v0.3.0-test.9", "draft": true, "assets": []},
  {"tag_name": "v0.1.0-test.2", "assets": [
     {"name": "downloadcraft-0.1.0-windows-x64.msi", "size": 9, "browser_download_url": "https://evil.example/x.msi"}]},
  {"tag_name": "nightly", "assets": []}
]"#;

#[test]
fn parses_releases_and_keeps_only_our_addresses() {
    let list = parse(LISTING, "-windows-x64.msi").unwrap();
    assert_eq!(list.len(), 2, "drafts and tags that aren't versions are left out");
    let r = &list[0];
    assert_eq!((r.tag.as_str(), r.version.as_str(), r.build, r.size), ("v0.2.0-test.7", "0.2.0", 7, 1234));
    assert!(r.download.ends_with("downloadcraft-0.2.0-windows-x64.msi"));
    assert_eq!(r.sha256, "a".repeat(64));
    assert!(r.sums.ends_with("SHA256SUMS.txt"));
    assert_eq!(r.commit.len(), 40);
    assert_eq!(list[1].download, "", "an address off our releases page is dropped");
    assert!(parse("{}", "x").is_err());
    assert!(parse("not json", "x").is_err());
}

#[test]
fn picks_only_a_newer_release() {
    let rel = |v: &str, build: u32, commit: &str| Release {
        tag: format!("v{v}-test.{build}"),
        version: v.into(),
        build,
        commit: commit.into(),
        ..Default::default()
    };
    let list = [rel("0.2.0", 7, "bbbbbbbbbb"), rel("0.2.0", 5, "aaaaaaaaaa"), rel("0.1.0", 3, "")];
    assert_eq!(newer_release("0.1.0", None, &list).map(|r| r.build), Some(7));
    assert_eq!(newer_release("0.3.0", None, &list), None);
    // The same version: only when this build is known to be the earlier test build.
    assert_eq!(newer_release("0.2.0", None, &list), None);
    assert_eq!(newer_release("0.2.0", Some("aaaaaaaaaa"), &list).map(|r| r.build), Some(7));
    assert_eq!(newer_release("0.2.0", Some("bbbbbbbbbb"), &list), None);
    assert_eq!(newer_release("junk", None, &list), None);
    assert_eq!(tag_version("v1.2.3"), Some(("1.2.3".into(), 0)));
    assert_eq!(tag_version("v1.2"), None);
}

#[test]
fn reads_checksum_files_and_names() {
    let sums = format!("{}  downloadcraft-0.2.0-windows-x64.msi\n{} *other.zip\n", "b".repeat(64), "c".repeat(64));
    assert_eq!(sum_for(&sums, "downloadcraft-0.2.0-windows-x64.msi"), Some("b".repeat(64)));
    assert_eq!(sum_for(&sums, "other.zip"), Some("c".repeat(64)));
    assert_eq!(sum_for(&sums, "missing"), None);
    assert_eq!(file_name("https://x/releases/download/v1/a-1.0.msi"), Some("a-1.0.msi"));
    assert_eq!(file_name("https://x/../.hidden"), None);
    assert_eq!(file_name("https://x/a%20b"), None);
}

#[test]
fn an_installer_download_carries_on_from_its_partial_file() {
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let (url, seen) = serve(Serve { body: body.clone(), ranges: true, cut_after: Some(100_000), ..Default::default() });
    let dir = std::env::temp_dir().join(format!("downloadcraft-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("x.msi.300000.part");
    let (progress, stop) = (Progress::default(), AtomicBool::new(false));
    assert!(fetch_part(&url, &part, 300_000, &progress, &stop).is_err(), "the first answer is cut short");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 100_000);
    assert_eq!(fetch_part(&url, &part, 300_000, &progress, &stop).unwrap(), Some(300_000));
    assert_eq!(std::fs::read(&part).unwrap(), body);
    assert_eq!(progress.get(), (300_000, 300_000));
    assert_eq!(seen.lock().unwrap().last().cloned().flatten().as_deref(), Some("100000"));
    let expected = sha256_file(&part).unwrap();
    assert_eq!(expected.len(), 64);
    let _ = std::fs::remove_dir_all(&dir);
}
