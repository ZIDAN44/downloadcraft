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
    for page in ["https://youtu.be/P6pcTK0wTYA?si=x", "https://www.youtube.com/watch?v=abc", "https://m.youtube.com/shorts/x", "https://vimeo.com/1"]
    {
        assert_eq!(classify(page), Ok(SourceKind::VideoPage), "{page}");
    }
    assert_eq!(classify("https://notyoutube.com/a.mp4"), Ok(SourceKind::Http), "only the site itself");
    assert!(SourceKind::VideoPage.not_yet().is_none(), "video pages download through yt-dlp");
    assert_eq!(classify("http://[::1]:8080/x"), Ok(SourceKind::Http));
    for bad in ["", "   ", "example.com/a.zip", "file:///etc/passwd", "javascript:alert(1)", "https://", "https:///x", "magnet:", "a b://c"] {
        assert!(classify(bad).is_err(), "{bad:?}");
    }
    assert!(classify(&format!("https://e.com/{}", "a".repeat(20_000))).is_err());
}

/// Links copied from pages keep their spaces (a magnet link's name, a file name): they're sent as
/// `%20`, as a browser does. Found with a magnet link refused as "a link has no spaces".
#[test]
fn spaces_in_a_link_are_sent_encoded() {
    use crate::source::normalize;
    let magnet = "magnet:?xt=urn:btih:2ee672984434179aa8a7c5d2ebd0a94b38eb2960&dn=Big Buck Bunny [1080p]&tr=udp://tracker.example:6969/announce";
    assert_eq!(classify(magnet), Ok(SourceKind::Magnet));
    assert_eq!(normalize(magnet), magnet.replace(' ', "%20"));
    assert_eq!(classify("https://e.com/my file.zip"), Ok(SourceKind::Http));
    assert_eq!(normalize("  https://e.com/my file.zip "), "https://e.com/my%20file.zip");
    assert_eq!(normalize("https://e.com/a.zip"), "https://e.com/a.zip", "nothing to change");
    // A tab is still refused, and so is a space before the scheme's `:`.
    for bad in ["https://e.com/a\tb", "a b://c", "not a link"] {
        assert!(classify(bad).is_err(), "{bad:?}");
    }
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
    // Found by the `names` fuzz target: control characters cut to length left only underscores,
    // which a second pass turned into the fallback; and a reserved name at full length grew a byte.
    let replaced = format!("{}a///", "\u{0}".repeat(300));
    assert_eq!(sanitize(&replaced), "download");
    let con = format!("CON.{}", "x".repeat(196));
    assert_eq!(sanitize(&con).len(), crate::names::MAX_NAME);
    assert!(sanitize(&con).starts_with("_CON."));
    let nbsp = format!("{}\u{a0}{}", "a".repeat(199), "b".repeat(10));
    assert_eq!(sanitize(&nbsp), "a".repeat(199), "cut on a no-break space, which goes too");
    for s in [&replaced, &con, &long, &nbsp] {
        assert_eq!(sanitize(&sanitize(s)), sanitize(s));
    }
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
        segments: Vec::new(),
        video: None,
        missing: false,
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

#[test]
fn video_qualities_come_from_yt_dlp_s_answer() {
    let info = serde_json::json!({
        "title": "Big Buck Bunny: 60fps/4K",
        "formats": [
            {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a", "filesize": 9_000_000},
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "filesize": 5_000_000},
            {"format_id": "137", "ext": "mp4", "height": 1080, "fps": 30, "vcodec": "avc1", "acodec": "none", "filesize": 80_000_000},
            {"format_id": "299", "ext": "mp4", "height": 1080, "fps": 60, "vcodec": "avc1", "acodec": "none", "filesize_approx": 120_000_000.0},
            {"format_id": "136", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "none"},
            {"format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none"}
        ]
    });
    let q = crate::probe::qualities(&info);
    let labels: Vec<&str> = q.iter().map(|q| q.label.as_str()).collect();
    assert_eq!(labels, ["1080p60", "720p", "360p", "Audio only"]);
    assert_eq!(q[0].size, Some(125_000_000), "the biggest 1080p stream and the audio");
    assert!(q[0].needs_ffmpeg && q[0].selector.contains("height<=1080"));
    assert!(!q[2].needs_ffmpeg, "360p comes as one file");
    assert_eq!(q[2].size, Some(9_000_000));
    assert_eq!(q[3].size, Some(5_000_000));
    let (title, name) = crate::probe::title_and_name(&info);
    assert_eq!(title, "Big Buck Bunny: 60fps/4K");
    assert_eq!(name, "Big Buck Bunny_ 60fps_4K.mp4");
    assert_eq!(crate::probe::qualities(&serde_json::json!({})).first().map(|q| q.selector.as_str()), Some(crate::probe::BEST));
    assert!(crate::probe::playlist(&info).is_none(), "a single video");
}

#[test]
fn subtitle_languages_become_ytdlp_languages() {
    use crate::probe::subtitle_langs;
    assert_eq!(subtitle_langs(""), None);
    assert_eq!(subtitle_langs(" , ; "), None);
    assert_eq!(subtitle_langs("en"), Some("en.*,-live_chat".into()));
    assert_eq!(subtitle_langs("EN, pt-BR;bn"), Some("en.*,pt-br.*,bn.*,-live_chat".into()));
    assert_eq!(subtitle_langs("en,all,-live_chat,../x,e"), Some("en.*,all.*,-live_chat".into()), "codes only");
    assert_eq!(subtitle_langs(&"en ".repeat(20)).map(|s| s.matches("en.*").count()), Some(8));
}

#[test]
fn youtube_links_want_a_javascript_runtime() {
    use crate::source::needs_js_runtime;
    for yt in ["https://www.youtube.com/watch?v=a", "https://youtu.be/a", "https://m.youtube.com/@x", "https://music.youtube.com/watch?v=a"] {
        assert!(needs_js_runtime(yt), "{yt}");
    }
    for other in ["https://vimeo.com/1", "https://notyoutube.com/watch?v=a", "https://youtube.com.evil.example/a", "youtube.com"] {
        assert!(!needs_js_runtime(other), "{other}");
    }
}

#[test]
fn playlist_links_are_told_from_video_links() {
    use crate::source::may_be_playlist;
    for list in [
        "https://www.youtube.com/playlist?list=PL123",
        "https://www.youtube.com/@channel/videos",
        "https://www.youtube.com/channel/UC1",
        "https://vimeo.com/showcase/42",
    ] {
        assert!(may_be_playlist(list), "{list}");
    }
    for one in ["https://www.youtube.com/watch?v=a1&list=PL123", "https://youtu.be/a1", "https://vimeo.com/42", "not a link"] {
        assert!(!may_be_playlist(one), "{one}");
    }
}

#[test]
fn playlists_list_their_videos() {
    // As `yt-dlp --flat-playlist -J` answers for a playlist.
    let info = serde_json::json!({
        "_type": "playlist", "title": "Talks: 2026",
        "entries": [
            {"_type": "url", "ie_key": "Youtube", "id": "a1", "url": "https://www.youtube.com/watch?v=a1", "title": "One / two"},
            {"_type": "url", "ie_key": "Youtube", "id": "b2", "url": "https://www.youtube.com/watch?v=b2", "title": "[Private video]"},
            {"_type": "url", "ie_key": "Youtube", "id": "c3", "url": "https://www.youtube.com/watch?v=c3"},
            {"_type": "url", "ie_key": "YoutubeTab", "url": "https://www.youtube.com/@x/shorts", "title": "Shorts"},
            {"_type": "url", "url": "file:///etc/passwd", "title": "not a page"}
        ]
    });
    let (title, videos) = crate::probe::playlist(&info).unwrap();
    assert_eq!(title, "Talks: 2026");
    let got: Vec<(&str, &str)> = videos.iter().map(|v| (v.url.as_str(), v.file_name.as_str())).collect();
    assert_eq!(got, [("https://www.youtube.com/watch?v=a1", "One _ two.mp4"), ("https://www.youtube.com/watch?v=c3", "c3.mp4")]);
    let q = crate::probe::playlist_qualities();
    assert_eq!(q.first().map(|q| q.selector.as_str()), Some(crate::probe::BEST));
    assert!(q.iter().any(|q| !q.needs_ffmpeg && q.selector == "b"), "one choice works without FFmpeg");

    // A channel's page: its tabs, each a playlist; live and coming streams left out, each video once.
    let channel = serde_json::json!({
        "_type": "playlist", "title": "NASA",
        "entries": [
            {"_type": "playlist", "title": "NASA - Videos", "entries": [
                {"_type": "url", "ie_key": "Youtube", "url": "https://www.youtube.com/watch?v=v1", "title": "Launch"}
            ]},
            {"_type": "playlist", "title": "NASA - Live", "entries": [
                {"_type": "url", "ie_key": "Youtube", "url": "https://www.youtube.com/watch?v=l1", "title": "Now", "live_status": "is_live"},
                {"_type": "url", "ie_key": "Youtube", "url": "https://www.youtube.com/watch?v=l2", "title": "Soon", "live_status": "is_upcoming"},
                {"_type": "url", "ie_key": "Youtube", "url": "https://www.youtube.com/watch?v=l3", "title": "Earlier", "live_status": "was_live"},
                {"_type": "url", "ie_key": "Youtube", "url": "https://www.youtube.com/watch?v=v1", "title": "Launch"}
            ]}
        ]
    });
    let (_, videos) = crate::probe::playlist(&channel).unwrap();
    let urls: Vec<&str> = videos.iter().map(|v| v.url.as_str()).collect();
    assert_eq!(urls, ["https://www.youtube.com/watch?v=v1", "https://www.youtube.com/watch?v=l3"]);
}
