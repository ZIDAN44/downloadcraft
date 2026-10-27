use std::collections::BTreeSet;

use super::*;

/// The UI crate's own Rust files, read from disk.
fn ui_sources() -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<std::path::PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() {
                if name != "i18n" {
                    walk(&p, out);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" && name != "i18n.rs" {
                out.push(std::fs::read_to_string(&p).unwrap_or_default());
            }
        }
    }
    let mut out = Vec::new();
    walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    assert!(out.len() > 8, "the UI's sources weren't found");
    out
}

/// Strings drawn through a variable, which the scan below can't see.
const THROUGH_VARIABLES: &[&str] = &[
    // The empty list.
    "No matches",
    "Nothing in this list has that name or link.",
    "No downloads yet",
    "Paste a link anywhere in this window with Ctrl+V, or add one.",
    "Nothing here",
    "Downloads in this category show up here.",
    // A row's buttons and menu.
    "Show in folder",
    "Open folder",
    // Remove, and Add link's button.
    "Remove and delete",
    "Add download",
    // The video tools.
    "Video links use yt-dlp, a free, open-source program that finds the video in a page. DownloadCraft fetches its official build and checks it before use.",
    "Get yt-dlp",
    "YouTube also needs Deno, a free JavaScript runtime yt-dlp reads its pages with; without it some qualities are missing. DownloadCraft fetches its official build and checks it before use.",
    "Get Deno",
    "Higher qualities come as separate video and sound, which FFmpeg joins into one file.",
    "Get FFmpeg",
    "Try again",
    // The details pane's tabs.
    "General",
    "Trackers",
    "Peers",
    "Files",
    // A torrent proxy that can't be used (`torrent::proxy_url`).
    "a proxy with a password isn't supported yet",
    "write the proxy as host:port",
    "the proxy's port is a number from 1 to 65535",
    "the proxy's host isn't a name or an address",
];

/// Every interface string: literals in the UI code (`tl!`, `tf!` and the helpers that translate
/// their label), the names the model hands up, and those drawn through variables.
pub(crate) fn interface_strings() -> BTreeSet<String> {
    let sources = ui_sources();
    // (prefix, which string literals after it on its line are interface text)
    let prefixes: [(&str, &[usize]); 13] = [
        ("tl!(", &[0]),
        ("tf!(", &[0]),
        ("button(ui, t, Kind::", &[0]),
        ("nav_row(ui, t, Icon::", &[0]),
        ("field_label(ui, t, ", &[0]),
        ("section(ui, t, ", &[0]),
        ("text_field(ui, ", &[0]),
        ("header(ui, t, ", &[0, 1]),
        ("setting(ui, t, ", &[0, 1]),
        // The details pane: a label beside its value, and a table's id then its column heads.
        ("pair(ui, t, ", &[0]),
        ("table(ui, t, ", &[1, 2, 3, 4, 5, 6, 7]),
        ("Icon::Remove, ", &[0]),
        // Labels in tables drawn in a loop: `(Icon::Pause, "Pause", Some("download.pause"))`.
        ("Icon::", &[0]),
    ];
    let mut out = BTreeSet::new();
    for src in sources.iter().map(String::as_str) {
        for (p, which) in prefixes {
            let mut rest = src;
            while let Some(i) = rest.find(p) {
                rest = &rest[i + p.len()..];
                // String literals up to the end of the line (the next line, when rustfmt put the
                // text there).
                let mut line = &rest[..rest.find('\n').unwrap_or(rest.len())];
                if line.trim().is_empty()
                    && let Some(next) = rest.get(line.len() + 1..)
                {
                    line = &next[..next.find('\n').unwrap_or(next.len())];
                }
                let mut lits: Vec<String> = Vec::new();
                let mut chars = line.chars();
                while let Some(c) = chars.next() {
                    if c != '"' {
                        continue;
                    }
                    let mut lit = String::new();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                if let Some(e) = chars.next() {
                                    lit.push(if e == 'n' { '\n' } else { e });
                                }
                            }
                            '"' => break,
                            c => lit.push(c),
                        }
                    }
                    lits.push(lit);
                }
                for &w in which {
                    // Command ids (`download.again`) and ids for egui aren't wording.
                    if let Some(l) = lits.get(w)
                        && l.chars().filter(|c| c.is_alphabetic()).count() >= 2
                        && !l.contains('.') | l.contains(' ')
                    {
                        out.insert(l.clone());
                    }
                }
            }
        }
    }
    use downloadcraft_engine::Filter;
    use downloadcraft_engine::model::{SourceKind, State, Theme, probe};
    out.extend(Filter::ALL.iter().map(|f| f.label().to_string()));
    out.extend([State::Queued, State::Downloading, State::Paused, State::Completed, State::Failed].iter().map(|s| s.label().to_string()));
    out.extend(Theme::ALL.iter().map(|t| t.label().to_string()));
    for k in [SourceKind::Http, SourceKind::Ftp, SourceKind::Torrent, SourceKind::Magnet, SourceKind::Mega, SourceKind::VideoPage] {
        out.insert(k.label().to_string());
        out.extend(k.not_yet().map(str::to_string));
    }
    let qualities = probe::playlist_qualities().into_iter().chain(probe::qualities(&serde_json::json!({}))).chain(probe::qualities(
        &serde_json::json!({"formats": [{"height": 720, "vcodec": "avc1", "acodec": "none"}, {"vcodec": "none", "acodec": "mp4a"}]}),
    ));
    out.extend(qualities.map(|q| q.label).filter(|l| l.chars().filter(|c| c.is_alphabetic()).count() >= 2));
    out.extend(THROUGH_VARIABLES.iter().map(|s| (*s).to_string()));
    // A row's state, with its context (`tc("state", …)`).
    out.extend(
        [State::Queued, State::Downloading, State::Paused, State::Completed, State::Failed].iter().map(|s| format!("state{CTX}{}", s.label())),
    );
    out
}

#[test]
fn catalogs_parse_cleanly() {
    for l in LANGUAGES.iter().skip(1) {
        let (entries, errors) = parse(l.source);
        assert!(errors.is_empty(), "{}: {errors:?}", l.code);
        assert!(entries.len() > 100, "{}: only {} entries", l.code, entries.len());
    }
}

#[test]
fn every_interface_string_is_translated() {
    let strings = interface_strings();
    if let Ok(path) = std::env::var("DOWNLOADCRAFT_DUMP_STRINGS") {
        let _ = std::fs::write(path, strings.iter().map(|s| s.replace('\n', "\\n")).collect::<Vec<_>>().join("\n"));
    }
    assert!(strings.len() > 100, "{} strings found", strings.len());
    for l in Lang::all().skip(1) {
        let missing: Vec<&String> = strings.iter().filter(|s| l.0.catalog().get(s.as_str()).is_none()).collect();
        assert!(missing.is_empty(), "{}: {} untranslated, e.g. {:?}", l.code(), missing.len(), missing.iter().take(12).collect::<Vec<_>>());
    }
}

/// Every catalog row is for a string the interface still draws (a row left behind by a change
/// in wording is never shown).
#[test]
fn catalogs_hold_no_stale_rows() {
    let strings = interface_strings();
    for l in LANGUAGES.iter().skip(1) {
        let (entries, _) = parse(l.source);
        let stale: Vec<&String> = entries.iter().map(|(k, _)| k).filter(|k| !strings.contains(*k)).collect();
        assert!(stale.is_empty(), "{}: rows the interface no longer draws: {stale:?}", l.code);
    }
}

#[test]
fn tags_and_templates() {
    assert_eq!(lang_from_tag("pt_BR.UTF-8").map(Lang::code), Some("pt-br"));
    assert_eq!(lang_from_tag("de-AT").map(Lang::code), Some("de"));
    assert_eq!(lang_from_tag("pt-PT"), None, "no European Portuguese catalog");
    assert_eq!(lang_from_tag("C").map(Lang::code), Some("en"));
    assert_eq!(lang_from_tag("ja-JP"), None);
    assert_eq!(Lang::from_pref("auto"), Lang::EN, "the system's, English in tests");
    assert_eq!(Lang::from_pref("RU").code(), "ru");
    assert_eq!(fmt("{n} of {total}", &[("n", "3"), ("total", "9")]), "3 of 9");
    assert_eq!(fmt("{n} {missing} {", &[("n", "{total}")]), "{total} {missing} {", "values aren't read as templates");
    set_current(Lang::from_code("de").unwrap_or(Lang::EN));
    assert_eq!(tfmt("{n} links", &[("n", "4")]), "4 Links");
    assert_eq!(t("Pause"), "Pausieren");
    set_current(Lang::EN);
    assert_eq!(t("Pause"), "Pause");
}

#[test]
fn a_tour_in_the_pseudo_language_only_draws_known_strings() {
    let known = interface_strings();
    pseudo::set(true);
    let ctx = egui::Context::default();
    let mut session = crate::tests::sample();
    crate::tests::add_torrent(&mut session);
    let mut app = crate::DownloadApp::new(session, Box::new(crate::NoServices));
    for _ in 0..2 {
        crate::tests::frame(&mut app, &ctx, vec![]);
    }
    for tab in [crate::details::Tab::General, crate::details::Tab::Trackers, crate::details::Tab::Peers, crate::details::Tab::Files] {
        app.details.tab = tab;
        crate::tests::frame(&mut app, &ctx, vec![]);
    }
    for f in ["active", "paused", "completed", "failed", "all"] {
        app.run("list.filter", serde_json::json!({"filter": f}));
        crate::tests::frame(&mut app, &ctx, vec![]);
    }
    app.open_add("https://e.com/a.zip\nmagnet:?xt=urn:btih:abc\nnot a link\nhttps://www.youtube.com/watch?v=x");
    crate::tests::frame(&mut app, &ctx, vec![]);
    app.open_settings();
    crate::tests::frame(&mut app, &ctx, vec![]);
    app.dialog = None;
    app.ask_remove(app.session.downloads.first().map(|d| d.id));
    crate::tests::frame(&mut app, &ctx, vec![]);
    app.dialog = None;
    app.update = crate::UpdateState::Ready { version: "9.9.9".into() };
    crate::tests::frame(&mut app, &ctx, vec![]);
    pseudo::set(false);
    // Text with values (names, sizes, errors) goes through `t` too; only fixed wording must be
    // known, and what was drawn and is written as a literal in the code is wording.
    let sources = ui_sources().concat();
    let unknown: Vec<String> = pseudo::seen()
        .into_iter()
        .filter(|s| s.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !known.contains(s) && sources.contains(&format!("\"{s}\"")))
        .collect();
    assert!(unknown.is_empty(), "drawn, written in the code, but not in the catalogs: {unknown:?}");
    assert!(pseudo::seen().len() > 40, "the tour drew {} strings", pseudo::seen().len());
}
