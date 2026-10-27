//! Looking at a link before it's added, for the name its file will get and its size, the way
//! browsers and download managers name downloads:
//!
//! 1. ask for the first byte (`Range: bytes=0-0`, a GET: some servers answer HEAD wrongly or not
//!    at all), following redirects;
//! 2. the name is `Content-Disposition`'s `filename*`, else its `filename` (RFC 6266), else the last
//!    part of the address after the redirects, else of the address given;
//! 3. a name without an extension gets one from `Content-Type` when that is a known kind of file;
//! 4. a `text/html` answer is a web page, not a file, and says so;
//! 5. the size is `Content-Range`'s total, else `Content-Length`.
//!
//! Every name is made safe for the file system (`names::sanitize`).

use std::time::Duration;

use downloadcraft_model::names;
use downloadcraft_model::probe::Probe;
use ureq::ResponseExt;

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .max_redirects(10)
        .user_agent(concat!("DownloadCraft/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

/// The extension for a `Content-Type`, for names that have none.
pub fn extension_for(content_type: &str) -> Option<&'static str> {
    let t = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    Some(match t.as_str() {
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/pdf" => "pdf",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-xz" => "xz",
        "application/x-tar" => "tar",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-msi" | "application/x-ms-installer" => "msi",
        "application/vnd.debian.binary-package" => "deb",
        "application/x-apple-diskimage" => "dmg",
        "application/x-iso9660-image" => "iso",
        "application/json" => "json",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "text/plain" => "txt",
        "text/csv" => "csv",
        _ => return None,
    })
}

/// The name a download of `url` gets, from what the server answered.
pub fn name_from(url: &str, final_url: Option<&str>, disposition: Option<&str>, content_type: Option<&str>) -> String {
    let mut name = disposition
        .and_then(names::from_disposition)
        .or_else(|| final_url.map(names::from_url).filter(|n| n != names::FALLBACK))
        .unwrap_or_else(|| names::from_url(url));
    let has_ext = name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && (1..=8).contains(&ext.len()));
    if !has_ext && let Some(ext) = content_type.and_then(extension_for) {
        name = names::sanitize(&format!("{name}.{ext}"));
    }
    name
}

/// `bytes 0-0/12345` → 12345.
fn range_total(v: &str) -> Option<u64> {
    v.rsplit_once('/')?.1.trim().parse().ok()
}

/// Look at a plain (non-video) link.
pub fn probe_file(url: &str) -> Probe {
    let resp = agent().get(url).header("Range", "bytes=0-0").header("Accept-Encoding", "identity").call();
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Probe::Failed(format!("couldn't reach it ({e})")),
    };
    let status = resp.status().as_u16();
    let header = |k: &str| resp.headers().get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    if !(200..300).contains(&status) {
        return Probe::Failed(match status {
            401 | 403 => format!("the server refused ({status}); the link may need a sign-in"),
            404 | 410 => format!("there's no file there ({status})"),
            _ => format!("the server answered {status}"),
        });
    }
    let final_url = resp.get_uri().to_string();
    let content_type = header("content-type");
    let name = name_from(url, Some(&final_url), header("content-disposition").as_deref(), content_type.as_deref());
    let size = if status == 206 {
        header("content-range").as_deref().and_then(range_total)
    } else {
        header("content-length").and_then(|l| l.trim().parse().ok())
    };
    let page = content_type.as_deref().is_some_and(|t| t.to_ascii_lowercase().starts_with("text/html"));
    let note = page.then(|| "this link opens a web page, not a file".to_string());
    Probe::File { name, size, note }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_header_then_the_final_address() {
        assert_eq!(name_from("https://e.com/get?id=4", None, Some(r#"attachment; filename="Report Q3.pdf""#), None), "Report Q3.pdf");
        assert_eq!(name_from("https://e.com/get?id=4", Some("https://cdn.e.com/files/setup-2.1.exe"), None, None), "setup-2.1.exe");
        assert_eq!(name_from("https://e.com/download/latest", Some("https://e.com/download/latest"), None, Some("application/zip")), "latest.zip");
        assert_eq!(name_from("https://e.com/", Some("https://e.com/"), None, Some("text/html; charset=utf-8")), "download");
        assert_eq!(name_from("https://e.com/a.tar.gz", None, None, Some("application/gzip")), "a.tar.gz", "an extension stays");
        assert_eq!(range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(range_total("bytes 0-0/*"), None);
    }
}
