use crate::names::{from_disposition, from_url, numbered, sanitize};
use crate::source::classify;
use crate::{Download, DownloadId, SourceKind, State, format};

#[test]
fn classifies_links() {
    assert_eq!(classify("https://example.com/a.zip"), Ok(SourceKind::Http));
    assert_eq!(classify("  HTTP://Example.com/a.iso?x=1  "), Ok(SourceKind::Http));
    assert_eq!(classify("https://example.com/linux.TORRENT"), Ok(SourceKind::Torrent));
    assert_eq!(classify("https://example.com/a.torrent?dl=1"), Ok(SourceKind::Torrent));
    assert_eq!(classify("magnet:?xt=urn:btih:abc"), Ok(SourceKind::Magnet));
    assert_eq!(classify("https://mega.nz/file/abc#key"), Ok(SourceKind::Mega));
    assert_eq!(classify("https://user@MEGA.NZ:443/folder/x"), Ok(SourceKind::Mega));
    assert_eq!(classify("ftp://ftp.example.org/pub/x.tar"), Ok(SourceKind::Ftp));
    assert_eq!(classify("http://[::1]:8080/x"), Ok(SourceKind::Http));
    for bad in ["", "   ", "example.com/a.zip", "file:///etc/passwd", "javascript:alert(1)", "https://", "https:///x", "magnet:", "a b://c"] {
        assert!(classify(bad).is_err(), "{bad:?}");
    }
    assert!(classify(&format!("https://e.com/{}", "a".repeat(20_000))).is_err());
}

#[test]
fn names_from_links() {
    assert_eq!(from_url("https://example.com/files/My%20Movie.mkv?token=1#x"), "My Movie.mkv");
    assert_eq!(from_url("https://example.com/dir/"), "dir");
    assert_eq!(from_url("https://example.com"), "download");
    assert_eq!(from_url("https://example.com/..%2F..%2Fetc%2Fpasswd"), "_.._etc_passwd");
    assert_eq!(from_url("https://e.com/%E4%B8%AD%E6%96%87.txt"), "中文.txt");
    assert_eq!(from_url("https://e.com/%ZZbad%"), "%ZZbad%");
}

#[test]
fn names_from_disposition() {
    assert_eq!(from_disposition(r#"attachment; filename="report.pdf""#).as_deref(), Some("report.pdf"));
    assert_eq!(from_disposition("attachment; filename=plain.txt").as_deref(), Some("plain.txt"));
    assert_eq!(from_disposition(r#"attachment; filename="a.txt"; filename*=UTF-8''%E2%82%AC%20rates.txt"#).as_deref(), Some("€ rates.txt"));
    assert_eq!(from_disposition(r#"attachment; filename="../../evil.exe""#).as_deref(), Some("_.._evil.exe"));
    assert_eq!(from_disposition("inline"), None);
    assert_eq!(from_disposition(r#"attachment; filename="""#), None);
}

#[test]
fn sanitizes_for_every_file_system() {
    assert_eq!(sanitize("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
    assert_eq!(sanitize("  ..hidden.  "), "hidden");
    assert_eq!(sanitize("CON"), "_CON");
    assert_eq!(sanitize("nul.txt"), "_nul.txt");
    assert_eq!(sanitize("COM1.log"), "_COM1.log");
    assert_eq!(sanitize("COMPUTER.txt"), "COMPUTER.txt");
    assert_eq!(sanitize("\u{0}\u{7}"), "download");
    let long = format!("{}.zip", "é".repeat(300));
    let s = sanitize(&long);
    assert!(s.len() <= crate::names::MAX_NAME, "{}", s.len());
    assert!(s.ends_with(".zip"));
    assert_eq!(numbered("a.tar.gz", 2), "a.tar (2).gz");
    assert_eq!(numbered("README", 1), "README (1)");
    assert_eq!(numbered(".bashrc", 1), ".bashrc (1)");
}

#[test]
fn formats() {
    assert_eq!(format::bytes(0), "0 bytes");
    assert_eq!(format::bytes(1), "1 byte");
    assert_eq!(format::bytes(1500), "1.50 KB");
    assert_eq!(format::bytes(73_400_000), "73.4 MB");
    assert_eq!(format::bytes(u64::MAX), "18447 PB");
    assert_eq!(format::speed(512), "512 B/s");
    assert_eq!(format::speed(2_310_000), "2.31 MB/s");
    assert_eq!(format::duration(45), "45s");
    assert_eq!(format::duration(725), "12m 05s");
    assert_eq!(format::duration(12_000), "3h 20m");
    assert_eq!(format::duration(187_200), "2d 4h");
}

#[test]
fn progress_and_eta() {
    let mut d = Download {
        id: DownloadId(1),
        url: "https://e.com/a".into(),
        kind: SourceKind::Http,
        file_name: "a".into(),
        folder: ".".into(),
        total: Some(1000),
        done: 250,
        state: State::Downloading,
        error: None,
        resumable: true,
        added: 0,
        speed: 100,
        run: 1,
    };
    assert_eq!(d.fraction(), Some(0.25));
    assert_eq!(d.eta(), Some(8));
    d.done = 5000; // a server that sent more than it said
    assert_eq!(d.fraction(), Some(1.0));
    assert_eq!(d.eta(), None);
    d.total = None;
    assert_eq!(d.fraction(), None);
    let json = serde_json::to_string(&d).unwrap_or_default();
    assert!(!json.contains("speed"), "{json}");
}
