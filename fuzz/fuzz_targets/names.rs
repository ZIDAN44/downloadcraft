//! What a server and a user hand over as names: `Content-Disposition`, links pasted or opened, and
//! the file names made of them. Whatever comes in, a name is safe as one file name everywhere.
#![no_main]

use downloadcraft_model::names::{self, FALLBACK, MAX_NAME};
use downloadcraft_model::source;
use libfuzzer_sys::fuzz_target;

/// The promise `names::sanitize` makes.
fn assert_safe(n: &str) {
    assert!(!n.is_empty(), "empty");
    assert!(n.len() <= MAX_NAME, "{} bytes", n.len());
    assert!(!n.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')), "{n:?}");
    assert!(!n.ends_with(['.', ' ']), "{n:?}");
    assert!(!n.starts_with(['.', ' ']), "{n:?}");
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let clean = names::sanitize(text);
    assert_safe(&clean);
    assert_eq!(names::sanitize(&clean), clean, "sanitizing twice changes nothing");
    if let Some(n) = names::from_disposition(text) {
        assert_safe(&n);
        assert_ne!(n, FALLBACK);
    }
    assert_safe(&names::sanitize(&names::from_url(text)));
    let _ = names::percent_decode(text);
    let _ = names::numbered(&clean, u32::MAX);
    let _ = source::classify(text);
    let _ = downloadcraft_net::probe::name_from(text, Some(text), Some(text), Some(text));
    let _ = downloadcraft_net::probe::extension_for(text);
});
