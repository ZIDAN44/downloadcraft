//! What a server and a damaged disk hand back during a download: `Content-Range`, the parts
//! record beside a partial file, and what the updater reads from GitHub (the release list, a
//! `SHA256SUMS.txt`, tags and asset links).
#![no_main]

use downloadcraft_net::http::{content_range, segmented};
use downloadcraft_net::updates;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Some((start, Some(total))) = content_range(text) {
        let _ = (start, total);
    }
    // A record that's let through covers the file exactly once, every part within its bounds.
    if let Some((total, parts)) = segmented::parse_record(text) {
        let mut at = 0;
        for p in &parts {
            assert!(p.start == at && p.start <= p.pos && p.pos <= p.end && p.end <= total, "{p:?} in {text}");
            at = p.end;
        }
        assert_eq!(at, total);
    }
    if let Ok(list) = updates::parse(text, "-windows-x64.msi") {
        let _ = updates::newer_release("0.2.0", None, &list);
        for r in &list {
            let _ = updates::expected_sum(r);
        }
    }
    let _ = updates::sum_for(text, "downloadcraft-0.2.0-windows-x64.msi");
    let _ = updates::tag_version(text);
    let _ = updates::file_name(text);
});
