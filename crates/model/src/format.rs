//! Sizes, speeds and times as the list writes them.

/// `1.4 GB`, `820 KB`, `12 bytes` (decimal units, as file managers show them).
pub fn bytes(n: u64) -> String {
    const UNITS: &[&str] = &["KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return if n == 1 { "1 byte".into() } else { format!("{n} bytes") };
    }
    let mut v = n as f64;
    let mut unit = "KB";
    for u in UNITS {
        v /= 1000.0;
        unit = u;
        if v < 1000.0 {
            break;
        }
    }
    if v < 10.0 {
        format!("{v:.2} {unit}")
    } else if v < 100.0 {
        format!("{v:.1} {unit}")
    } else {
        format!("{v:.0} {unit}")
    }
}

/// `2.31 MB/s`.
pub fn speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec < 1000 { format!("{bytes_per_sec} B/s") } else { format!("{}/s", bytes(bytes_per_sec)) }
}

/// `45s`, `12m 05s`, `3h 20m`, `2d 4h`.
pub fn duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}
