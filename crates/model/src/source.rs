//! What kind of link the user pasted, and whether this version can fetch it.

use serde::{Deserialize, Serialize};

/// The kinds of link DownloadCraft knows. `Http` and `VideoPage` download; the others are
/// recognised so the app can say when they arrive (ROADMAP.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// A direct `http://` or `https://` link.
    Http,
    /// An `ftp://` or `ftps://` link.
    Ftp,
    /// A `.torrent` file's link.
    Torrent,
    /// A `magnet:` link (BEP 9).
    Magnet,
    /// A MEGA file or folder link (`mega.nz`, `mega.io`).
    Mega,
    /// A video site's page (YouTube, Vimeo…): a web page, not the video file, so it's fetched
    /// through yt-dlp, which finds the video in it.
    #[serde(rename = "videopage")]
    VideoPage,
}

/// Sites whose links are pages that play a video, not files.
const VIDEO_SITES: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "vimeo.com",
    "dailymotion.com",
    "twitch.tv",
    "tiktok.com",
    "instagram.com",
    "facebook.com",
    "fb.watch",
    "x.com",
    "twitter.com",
    "reddit.com",
    "bilibili.com",
];

/// Whether `host` is one of [`VIDEO_SITES`] or a part of one (`www.youtube.com`, `m.youtube.com`).
fn is_video_site(host: &str) -> bool {
    VIDEO_SITES.iter().any(|site| host == *site || host.strip_suffix(site).is_some_and(|sub| sub.ends_with('.')))
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Http => "Direct link",
            SourceKind::Ftp => "FTP",
            SourceKind::Torrent => "Torrent",
            SourceKind::Magnet => "Magnet link",
            SourceKind::Mega => "MEGA",
            SourceKind::VideoPage => "Video page",
        }
    }
    /// `None` when this version downloads it; otherwise why not yet.
    pub fn not_yet(self) -> Option<&'static str> {
        match self {
            SourceKind::Http => None,
            SourceKind::Ftp => Some("FTP downloads arrive in v0.5"),
            SourceKind::Torrent | SourceKind::Magnet => Some("torrents arrive in v0.4"),
            SourceKind::Mega => Some("MEGA links arrive in v0.6"),
            SourceKind::VideoPage => None,
        }
    }
}

/// The longest link accepted (servers rarely take more than 8 KiB; magnet links run longer).
pub const MAX_URL: usize = 16 * 1024;

/// What `text` links to, or why it isn't a link DownloadCraft can take.
pub fn classify(text: &str) -> Result<SourceKind, &'static str> {
    let url = normalize(text);
    let url = url.as_str();
    if url.is_empty() {
        return Err("paste a link");
    }
    if url.len() > MAX_URL {
        return Err("the link is too long");
    }
    if url.chars().any(char::is_whitespace) {
        return Err("a link has no spaces");
    }
    let Some((scheme, rest)) = url.split_once(':') else { return Err("that isn't a link") };
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "magnet" {
        return if rest.starts_with('?') { Ok(SourceKind::Magnet) } else { Err("that magnet link is incomplete") };
    }
    let Some(after) = rest.strip_prefix("//") else { return Err("that isn't a link") };
    let host = host_of(after);
    if host.is_empty() {
        return Err("the link has no server");
    }
    match scheme.as_str() {
        "http" | "https" => {
            if host == "mega.nz" || host == "mega.io" || host.ends_with(".mega.nz") {
                Ok(SourceKind::Mega)
            } else if is_video_site(&host) {
                Ok(SourceKind::VideoPage)
            } else if path_of(after).to_ascii_lowercase().ends_with(".torrent") {
                Ok(SourceKind::Torrent)
            } else {
                Ok(SourceKind::Http)
            }
        }
        "ftp" | "ftps" => Ok(SourceKind::Ftp),
        _ => Err("only http, https, ftp, magnet and MEGA links"),
    }
}

/// A pasted link as it's sent: trimmed, and with spaces after its scheme written `%20`, as a
/// browser sends them. Links copied from pages often keep them as typed (a magnet link's `dn=`
/// name, a file name in a path). Other whitespace, and a space before the `:`, stay, so the link
/// is refused.
pub fn normalize(text: &str) -> String {
    let url = text.trim();
    match url.split_once(':') {
        Some((scheme, rest)) if !scheme.contains(' ') && rest.contains(' ') => format!("{scheme}:{}", rest.replace(' ', "%20")),
        _ => url.to_string(),
    }
}

/// Whether a video page's link is YouTube's, whose pages yt-dlp reads with a JavaScript runtime
/// (Deno).
pub fn needs_js_runtime(url: &str) -> bool {
    let Some((_, rest)) = url.trim().split_once("//") else { return false };
    let host = host_of(rest);
    host == "youtu.be"
        || host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
}

/// Whether a video page's link may be a playlist or a channel rather than one video: a `list=`
/// without a `v=`, or a path naming a playlist, a channel or a user. A guess, used to decide whether
/// to look before downloading; the look (yt-dlp) says for sure.
pub fn may_be_playlist(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("//") else { return false };
    let lower = path_of(rest).to_ascii_lowercase();
    let query = rest.split_once('?').map_or("", |(_, q)| q).to_ascii_lowercase();
    let has = |k: &str| query.split('&').any(|p| p.starts_with(k));
    (has("list=") && !has("v="))
        || ["/playlist", "/channel/", "/c/", "/user/", "/@", "/videos", "/showcase/", "/album/"].iter().any(|p| lower.contains(p))
}

/// The host of `after` (what follows `scheme://`), lowercased, without user info or port.
fn host_of(after: &str) -> String {
    let authority = after.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host =
        if let Some(v6) = host_port.strip_prefix('[') { v6.split(']').next().unwrap_or("") } else { host_port.split(':').next().unwrap_or("") };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The path of `after`, without the query or fragment.
pub(crate) fn path_of(after: &str) -> &str {
    let no_frag = after.split('#').next().unwrap_or("");
    let no_query = no_frag.split('?').next().unwrap_or("");
    no_query.find('/').and_then(|i| no_query.get(i..)).unwrap_or("")
}
