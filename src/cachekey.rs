//! Cache keys that identify a *video*, not a URL.
//!
//! `youtu.be/X`, `youtube.com/watch?v=X&si=…` and `youtube.com/shorts/X` are
//! the same video. For well-known sites the id is parsed straight from the
//! link (no network, no yt-dlp). Everything else falls back to the
//! normalized URL until yt-dlp's metadata names the video (extractor + id).
//!
//! A key is `<video>|<tag>`:
//!   * request tags `a`, `v@max`, `v@720` — "what was asked for";
//!   * delivered tags `v=720`, `v=?` — "what was actually sent", so a request
//!     for 1080p of a 720p-only video, or "max" that had to fall back to 720p
//!     to fit the size limit, reuses the 720p upload.

use crate::types::{Kind, Quality};
use crate::util::{host_of, normalize_url};

/// `<site>:<id>` for links whose video id is visible in the URL. The site
/// names match yt-dlp's extractor keys (lowercased), so these keys line up
/// with the ones derived from metadata.
pub fn local_video_key(url: &str) -> Option<String> {
    let host = host_of(url)?;
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let rest = url.split_once("://")?.1;
    let path_and_query = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
    let path_and_query = path_and_query.split('#').next().unwrap_or("");
    let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let param =
        |name: &str| query.split('&').find_map(|p| p.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v));

    let (site, id) = match host.as_str() {
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            let id = match segs.as_slice() {
                ["watch", ..] => param("v"),
                ["shorts" | "embed" | "live" | "v" | "clip", id, ..] => Some(*id),
                _ => None,
            };
            ("youtube", id.filter(|id| is_youtube_id(id))?)
        }
        "youtu.be" => ("youtube", segs.first().copied().filter(|id| is_youtube_id(id))?),
        "tiktok.com" | "m.tiktok.com" => match segs.as_slice() {
            [user, "video" | "photo", id, ..] if user.starts_with('@') && is_digits(id) => ("tiktok", *id),
            _ => return None,
        },
        "instagram.com" => match segs.as_slice() {
            ["p" | "reel" | "reels" | "tv", code, ..] | [_, "p" | "reel", code, ..] if is_word(code) => {
                ("instagram", *code)
            }
            _ => return None,
        },
        "twitter.com" | "x.com" | "mobile.twitter.com" | "mobile.x.com" => match segs.as_slice() {
            [_, "status", id, ..] | ["i", "web", "status", id, ..] if is_digits(id) => ("twitter", *id),
            _ => return None,
        },
        "vimeo.com" | "player.vimeo.com" => {
            let id = segs.iter().rev().find(|s| is_digits(s))?;
            ("vimeo", *id)
        }
        "reddit.com" | "old.reddit.com" | "new.reddit.com" => match segs.as_slice() {
            ["r", _, "comments", id, ..] | ["comments", id, ..] if is_word(id) => ("reddit", *id),
            _ => return None,
        },
        "redd.it" => ("reddit", segs.first().copied().filter(|id| is_word(id))?),
        "vk.com" | "m.vk.com" | "vkvideo.ru" => {
            let raw = segs.first().copied().or_else(|| param("z"))?;
            let id = raw.strip_prefix("video").or_else(|| raw.strip_prefix("clip"))?;
            if is_vk_id(id) {
                ("vk", id)
            } else {
                return None;
            }
        }
        "rutube.ru" => match segs.as_slice() {
            ["video", id, ..] if is_word(id) => ("rutube", *id),
            _ => return None,
        },
        "coub.com" => match segs.as_slice() {
            ["view", id, ..] if is_word(id) => ("coub", *id),
            _ => return None,
        },
        "pinterest.com" | "pinterest.ru" => match segs.as_slice() {
            ["pin", id, ..] if is_digits(id) => ("pinterest", *id),
            _ => return None,
        },
        _ => return None,
    };
    Some(format!("{site}:{id}"))
}

fn is_vk_id(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let Some((owner, item)) = s.split_once('_') else { return false };
    is_digits(owner) && is_digits(item)
}

fn is_youtube_id(s: &str) -> bool {
    s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Best video identity knowable without the network.
pub fn video_key(url: &str) -> String {
    local_video_key(url).unwrap_or_else(|| format!("url:{}", normalize_url(url)))
}

/// Video identity from yt-dlp metadata.
pub fn meta_video_key(extractor_key: Option<&str>, id: Option<&str>) -> Option<String> {
    let (ex, id) = (extractor_key?.trim(), id?.trim());
    if ex.is_empty() || id.is_empty() || ex.eq_ignore_ascii_case("generic") {
        return None;
    }
    Some(format!("{}:{id}", ex.to_ascii_lowercase()))
}

/// "What was asked for."
pub fn request_key(video: &str, q: Quality) -> String {
    match q {
        Quality::Audio => format!("{video}|a"),
        Quality::Max => format!("{video}|v@max"),
        Quality::Height(h) => format!("{video}|v@{h}"),
    }
}

/// "What was actually sent."
pub fn delivered_key(video: &str, kind: Kind, height: Option<u32>) -> String {
    match (kind, height) {
        (Kind::Audio, _) => format!("{video}|a"),
        (Kind::Video, Some(h)) => format!("{video}|v={h}"),
        (Kind::Video, None) => format!("{video}|v=?"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_variants_are_one_video() {
        let id = Some("youtube:dQw4w9WgXcQ".to_string());
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=10s",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ#x",
            "https://youtu.be/dQw4w9WgXcQ?si=abcdef",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=RD",
            "HTTPS://WWW.YOUTUBE.COM/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(local_video_key(url), id, "{url}");
        }
        assert_eq!(local_video_key("https://www.youtube.com/watch?v=short"), None);
        assert_eq!(local_video_key("https://www.youtube.com/@channel"), None);
    }

    #[test]
    fn other_sites() {
        let cases = [
            (
                "https://www.tiktok.com/@user.name/video/7300000000000000001?is_from_webapp=1",
                Some("tiktok:7300000000000000001"),
            ),
            ("https://vm.tiktok.com/ZMabc/", None),
            ("https://www.instagram.com/reel/C1a-B2_c3/?igsh=x", Some("instagram:C1a-B2_c3")),
            ("https://instagram.com/p/C1aB2c3/", Some("instagram:C1aB2c3")),
            ("https://x.com/someone/status/1790000000000000000?s=20", Some("twitter:1790000000000000000")),
            ("https://twitter.com/i/web/status/1790000000000000000", Some("twitter:1790000000000000000")),
            ("https://vimeo.com/76979871", Some("vimeo:76979871")),
            ("https://player.vimeo.com/video/76979871?h=abc", Some("vimeo:76979871")),
            ("https://www.reddit.com/r/videos/comments/1abcde/some_title/", Some("reddit:1abcde")),
            ("https://redd.it/1abcde", Some("reddit:1abcde")),
            ("https://vk.com/video-123456_789012", Some("vk:-123456_789012")),
            ("https://vk.com/clip-123456_789012", Some("vk:-123456_789012")),
            ("https://vkvideo.ru/video-123456_789012?list=abc", Some("vk:-123456_789012")),
            (
                "https://rutube.ru/video/abcdef1234567890abcdef1234567890/",
                Some("rutube:abcdef1234567890abcdef1234567890"),
            ),
            ("https://coub.com/view/2v1a3b", Some("coub:2v1a3b")),
            ("https://www.pinterest.com/pin/123456789/", Some("pinterest:123456789")),
            ("https://example.com/watch/1", None),
        ];
        for (url, want) in cases {
            assert_eq!(local_video_key(url).as_deref(), want, "{url}");
        }
    }

    #[test]
    fn keys() {
        assert_eq!(video_key("https://example.com/v.mp4?utm_source=x"), "url:https://example.com/v.mp4");
        assert_eq!(meta_video_key(Some("Youtube"), Some("dQw4w9WgXcQ")).as_deref(), Some("youtube:dQw4w9WgXcQ"));
        assert_eq!(meta_video_key(Some("Generic"), Some("v")), None);
        assert_eq!(meta_video_key(None, Some("v")), None);
        assert_eq!(request_key("yt:x", Quality::Height(720)), "yt:x|v@720");
        assert_eq!(request_key("yt:x", Quality::Max), "yt:x|v@max");
        assert_eq!(delivered_key("yt:x", Kind::Video, Some(720)), "yt:x|v=720");
        assert_eq!(delivered_key("yt:x", Kind::Audio, None), request_key("yt:x", Quality::Audio));
    }
}
