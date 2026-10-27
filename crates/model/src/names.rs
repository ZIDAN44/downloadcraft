//! File names: from a link, from a server's `Content-Disposition`, and made safe for every
//! file system DownloadCraft runs on.

/// The longest file name kept, in bytes (most file systems allow 255).
pub const MAX_NAME: usize = 200;

/// What a file is called when nothing better is known.
pub const FALLBACK: &str = "download";

/// A file name from the link's last path segment (percent-decoded), else [`FALLBACK`].
pub fn from_url(url: &str) -> String {
    let after = url.split_once("://").map_or(url, |(_, a)| a);
    let path = crate::source::path_of(after);
    let last = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    sanitize(&percent_decode(last))
}

/// The file name in a `Content-Disposition` header (RFC 6266): `filename*=UTF-8''…` first, then
/// `filename="…"`. `None` when the header names none.
pub fn from_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        let Some((key, value)) = part.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if key == "filename*" {
            // charset'language'percent-encoded; only UTF-8 (and ASCII) are read.
            let mut bits = value.splitn(3, '\'');
            let charset = bits.next().unwrap_or("").to_ascii_lowercase();
            let encoded = bits.nth(1).unwrap_or("");
            if charset == "utf-8" || charset == "us-ascii" {
                let name = sanitize(&percent_decode(encoded));
                if name != FALLBACK {
                    return Some(name);
                }
            }
        } else if key == "filename" && plain.is_none() {
            let v = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
            plain = Some(sanitize(&v.replace("\\\"", "\"")));
        }
    }
    plain.filter(|n| n != FALLBACK)
}

/// `name` made safe as one file name everywhere: no path separators or control characters, none
/// of Windows' reserved characters or device names, no leading or trailing dots and spaces, at
/// most [`MAX_NAME`] bytes (cut on a character boundary, keeping the extension).
pub fn sanitize(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    // Checked after cutting to length: a long run of replaced characters can be all that's left.
    let mut out = cap(trimmed);
    if out.is_empty() || out.chars().all(|c| c == '_') {
        return FALLBACK.to_string();
    }
    let stem = out.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes().get(3).is_some_and(u8::is_ascii_digit);
    if RESERVED.contains(&stem.as_str()) || numbered {
        out = cap(&format!("_{out}"));
    }
    out
}

/// Whether `part`, one piece of a path inside a torrent, can't be written as it is: empty, `.` or
/// `..`, a path separator or control character, longer than 255 bytes, or, on Windows (`windows`),
/// one of its reserved characters (a `:` would open another stream of the file) or device names,
/// or ending in a dot or space. A torrent's files keep their names, so such a torrent is refused
/// rather than written somewhere else.
pub fn unsafe_part(part: &str, windows: bool) -> bool {
    if part.is_empty() || part == "." || part == ".." || part.len() > 255 || part.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return true;
    }
    if !windows {
        return false;
    }
    let stem = part.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes().get(3).is_some_and(u8::is_ascii_digit);
    part.contains([':', '*', '?', '"', '<', '>', '|'])
        || part.ends_with(['.', ' '])
        || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || numbered
}

/// Cut `name` to [`MAX_NAME`] bytes, keeping a short extension.
fn cap(name: &str) -> String {
    if name.len() <= MAX_NAME {
        return name.to_string();
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).filter(|e| e.len() <= 16 && !e.is_empty()).unwrap_or("");
    let room = MAX_NAME.saturating_sub(if ext.is_empty() { 0 } else { ext.len() + 1 });
    let mut end = room.min(name.len());
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    // Any whitespace, as `sanitize` trims: a cut can end on a no-break space (found by fuzzing).
    let stem = name.get(..end).unwrap_or("").trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if ext.is_empty() { stem.to_string() } else { format!("{stem}.{ext}") }
}

/// `name (n).ext`: the `n`th name to try when `name` is taken.
pub fn numbered(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

/// Decode `%XX` escapes; invalid escapes stay as written and invalid UTF-8 is replaced.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = |j: usize| b.get(j).and_then(|h| (*h as char).to_digit(16));
        match (c, hex(i + 1), hex(i + 2)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
