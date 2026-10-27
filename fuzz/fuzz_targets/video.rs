//! What yt-dlp prints: the `-J` description of a video page (its formats become the quality list
//! and its title the file name) and the progress lines of a download.
#![no_main]

use downloadcraft_model::names::MAX_NAME;
use downloadcraft_model::probe;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    for line in text.lines() {
        if let Some((done, total, _speed)) = downloadcraft_net::video::parse_progress(line) {
            let _ = total.map(|t| t.max(done));
        }
    }
    let Ok(info) = serde_json::from_str::<serde_json::Value>(text) else { return };
    for q in probe::qualities(&info) {
        assert!(!q.label.is_empty(), "{q:?}");
    }
    let (_title, name) = probe::title_and_name(&info);
    assert!(!name.is_empty() && name.len() <= MAX_NAME && !name.contains(['/', '\\']), "{name:?}");
});
