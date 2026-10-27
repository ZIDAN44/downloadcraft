//! What a link turns out to be before it's added: the file's real name and size, or a video page's
//! title and the qualities it comes in.
//!
//! The qualities are worked out here from yt-dlp's `-J` answer (pure data, so it's tested here):
//! one choice per height the page offers, best first, each as a yt-dlp format selector that takes
//! the best video at or under that height with the best audio, falling back to a single file with
//! both when there are no separate streams. Among the streams a selector allows, [`SORT`] (yt-dlp's
//! format sorting) prefers the highest resolution, then H.264 video and M4A audio, so the file plays
//! on nearly everything (AV1 and VP9 don't on many TVs, phones and older players).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One quality a video can be downloaded in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quality {
    /// `1080p`, `720p60`, `Audio only`.
    pub label: String,
    /// The yt-dlp format selector (`-f`).
    pub selector: String,
    /// About how big it will be, when the page says.
    pub size: Option<u64>,
    /// Video and audio come separately and need FFmpeg to be put together.
    pub needs_ffmpeg: bool,
}

/// What looking at a link found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Still asking.
    Pending,
    /// A file: its name as it will be saved, and its size when the server said.
    File {
        name: String,
        size: Option<u64>,
        note: Option<String>,
    },
    /// A video page: its title, the name the file will get, and the qualities, best first.
    Video {
        title: String,
        file_name: String,
        qualities: Vec<Quality>,
    },
    /// A playlist or channel page: its title, its videos, and the qualities offered for all of
    /// them ([`playlist_qualities`]: each video isn't looked at on its own).
    Playlist {
        title: String,
        videos: Vec<PlaylistVideo>,
        qualities: Vec<Quality>,
    },
    /// A torrent or magnet link: its name and the files inside, in the torrent's order.
    Torrent {
        name: String,
        files: Vec<TorrentFile>,
    },
    Failed(String),
}

/// One file inside a torrent: its place in the torrent's list (what `download.add`'s `files`
/// takes), its path within the torrent (`Season 1/01.mkv`) and its size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
}

/// The most files listed from one torrent (a torrent can name millions).
pub const MAX_TORRENT_FILES: usize = 10_000;

/// yt-dlp's `--sub-langs` for the languages typed in Settings (`en, pt-BR`): each code with its
/// regional variants (`en.*`), live chat left out; `None` when there are none. At most eight
/// languages; anything that isn't a language code is left out.
pub fn subtitle_langs(typed: &str) -> Option<String> {
    let codes: Vec<String> = typed
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(str::trim)
        .filter(|c| (2..=10).contains(&c.len()) && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
        .map(|c| format!("{}.*", c.to_ascii_lowercase()))
        .take(8)
        .collect();
    (!codes.is_empty()).then(|| format!("{},-live_chat", codes.join(",")))
}

/// One video of a playlist: its page and the name its file will get.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistVideo {
    pub url: String,
    pub file_name: String,
}

/// The most videos taken from one playlist.
pub const MAX_PLAYLIST: usize = 500;

/// Where one of the helper programs video downloads use stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToolState {
    #[default]
    Missing,
    Fetching,
    Ready,
    Failed(String),
}

/// yt-dlp (reads video pages), FFmpeg (puts video and audio together) and Deno (the JavaScript
/// runtime yt-dlp needs for YouTube).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoTools {
    pub ytdlp: ToolState,
    pub ffmpeg: ToolState,
    pub deno: ToolState,
}

/// One of the helper programs video downloads use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    Deno,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Ffmpeg => "FFmpeg",
            Tool::Deno => "Deno",
        }
    }
}

/// The selector for video at most `height` pixels tall with the best audio.
pub fn selector_for_height(height: u64) -> String {
    format!("bv*[height<={height}]+ba/b[height<={height}]")
}

/// The selector for the best there is.
pub const BEST: &str = "bv*+ba/b";
/// The selector for sound only.
pub const AUDIO_ONLY: &str = "ba/b";
/// yt-dlp's format sort (`-S`): resolution first, then H.264 video and M4A audio.
pub const SORT: &str = "res,vcodec:h264,acodec:m4a";

fn num(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u64)))
}

fn has(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_str).is_some_and(|c| c != "none")
}

fn size_of(v: &Value) -> Option<u64> {
    num(v, "filesize").or_else(|| num(v, "filesize_approx"))
}

/// The qualities in yt-dlp's `-J` answer for one video, best first, then Audio only.
pub fn qualities(info: &Value) -> Vec<Quality> {
    let formats: &[Value] = info.get("formats").and_then(Value::as_array).map_or(&[], |a| a.as_slice());
    let formats = formats.get(..formats.len().min(500)).unwrap_or_default();
    // The best audio stream on its own, to add to each video-only size.
    let audio = formats.iter().filter(|f| has(f, "acodec") && !has(f, "vcodec")).filter_map(size_of).max();
    let mut heights: Vec<(u64, u64)> = Vec::new();
    for f in formats {
        let Some(h) = num(f, "height").filter(|h| (100..=8640).contains(h)) else { continue };
        if !has(f, "vcodec") {
            continue;
        }
        let fps = num(f, "fps").unwrap_or(0);
        if let Some(e) = heights.iter_mut().find(|(eh, _)| *eh == h) {
            e.1 = e.1.max(fps);
        } else {
            heights.push((h, fps));
        }
    }
    heights.sort_by(|a, b| b.cmp(a));
    let mut out: Vec<Quality> = heights
        .into_iter()
        .take(12)
        .map(|(h, fps)| {
            // The video stream this choice is likely to take: the biggest at that height.
            let at: Vec<&Value> = formats.iter().filter(|f| num(f, "height") == Some(h) && has(f, "vcodec")).collect();
            let separate = at.iter().all(|f| !has(f, "acodec"));
            let video = at.iter().filter_map(|f| size_of(f)).max();
            let size = match (video, separate) {
                (Some(v), true) => Some(v.saturating_add(audio.unwrap_or(0))),
                (v, _) => v,
            };
            let label = if fps > 30 { format!("{h}p{fps}") } else { format!("{h}p") };
            Quality { label, selector: selector_for_height(h), size, needs_ffmpeg: separate }
        })
        .collect();
    if audio.is_some() || formats.iter().any(|f| has(f, "acodec")) {
        out.push(Quality { label: "Audio only".into(), selector: AUDIO_ONLY.into(), size: audio, needs_ffmpeg: false });
    }
    if out.is_empty() {
        out.push(Quality { label: "Best available".into(), selector: BEST.into(), size: None, needs_ffmpeg: false });
    }
    out
}

/// The qualities offered for a whole playlist: the best, common heights (each video gets the best
/// it has at or under it), one file with sound (no FFmpeg needed, often 360p), and sound only.
pub fn playlist_qualities() -> Vec<Quality> {
    let mut out = vec![Quality { label: "Best".into(), selector: BEST.into(), size: None, needs_ffmpeg: true }];
    for h in [1080, 720, 480] {
        out.push(Quality { label: format!("Up to {h}p"), selector: selector_for_height(h), size: None, needs_ffmpeg: true });
    }
    out.push(Quality { label: "One file with sound".into(), selector: "b".into(), size: None, needs_ffmpeg: false });
    out.push(Quality { label: "Audio only".into(), selector: AUDIO_ONLY.into(), size: None, needs_ffmpeg: false });
    out
}

/// A playlist in yt-dlp's `--flat-playlist -J` answer: its title and its videos (at most
/// [`MAX_PLAYLIST`], each once). A channel's page lists its tabs (Videos, Live, Shorts) as playlists
/// of their own, whose videos are taken in turn; private and deleted videos, streams that are live
/// or yet to come, and links to further playlists are left out. `None` when the answer is a single
/// video.
pub fn playlist(info: &Value) -> Option<(String, Vec<PlaylistVideo>)> {
    if info.get("_type").and_then(Value::as_str) != Some("playlist") {
        return None;
    }
    let title = info.get("title").and_then(Value::as_str).unwrap_or("Playlist").chars().take(180).collect::<String>();
    let mut videos = Vec::new();
    for e in entries(info) {
        if e.get("_type").and_then(Value::as_str) == Some("playlist") {
            // A tab: its videos, one level down only.
            entries(e).iter().for_each(|v| take_video(v, &mut videos));
        } else {
            take_video(e, &mut videos);
        }
    }
    Some((title, videos))
}

fn entries(v: &Value) -> &[Value] {
    v.get("entries").and_then(Value::as_array).map_or(&[], |a| a.as_slice())
}

/// Add the playlist entry `e` to `videos` when it's a video that can be downloaded.
fn take_video(e: &Value, videos: &mut Vec<PlaylistVideo>) {
    if videos.len() >= MAX_PLAYLIST {
        return;
    }
    let Some(url) = e.get("url").and_then(Value::as_str).filter(|u| u.starts_with("https://") || u.starts_with("http://")) else { return };
    let key = e.get("ie_key").and_then(Value::as_str).unwrap_or("");
    let nested = e.get("_type").and_then(Value::as_str) == Some("playlist") || key.ends_with("Tab") || key.ends_with("Playlist");
    let live = matches!(e.get("live_status").and_then(Value::as_str), Some("is_live" | "is_upcoming"));
    let name = e.get("title").and_then(Value::as_str).unwrap_or("");
    if nested || live || matches!(name, "[Private video]" | "[Deleted video]") || videos.iter().any(|v| v.url == url) {
        return;
    }
    let name = if name.is_empty() { e.get("id").and_then(Value::as_str).unwrap_or("video") } else { name };
    let name: String = name.chars().take(180).collect();
    videos.push(PlaylistVideo { url: url.chars().take(2048).collect(), file_name: crate::names::sanitize(&format!("{name}.mp4")) });
}

/// The video's title, and the name its file will get (`title.mp4`, made safe).
pub fn title_and_name(info: &Value) -> (String, String) {
    let title = info.get("title").and_then(Value::as_str).unwrap_or("video").chars().take(180).collect::<String>();
    let name = crate::names::sanitize(&format!("{title}.mp4"));
    (title, name)
}
