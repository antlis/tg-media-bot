//! Pure helpers: URL handling, formatting, sanitizing.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Pull up to `max` distinct http(s) URLs out of free text.
pub fn extract_urls(text: &str, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // ASCII lowercasing keeps byte offsets identical to `text`.
    let lower = text.to_ascii_lowercase();
    let mut pos = 0;
    while pos < text.len() && out.len() < max {
        let rest = &lower[pos..];
        let start = match (rest.find("http://"), rest.find("https://")) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => break,
        };
        let s = &text[pos + start..];
        let end = s
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '[' | ']' | '{' | '}' | '"' | '|' | '`'))
            .unwrap_or(s.len());
        let mut url = s[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', '\'']);
        if url.ends_with(')') && !url.contains('(') {
            url = url.trim_end_matches(')');
        }
        if is_valid_url(url) && !out.iter().any(|u| u == url) {
            out.push(url.to_string());
        }
        pos += start + end.max(1);
    }
    out
}

pub fn is_valid_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = match lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) {
        Some(r) => r,
        None => return false,
    };
    url.len() <= 2048 && !rest.is_empty() && !rest.starts_with('/') && host_of(url).is_some()
}

/// Lowercased host of an http(s) URL, without userinfo and port.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let hostport = authority.rsplit('@').next()?;
    let host =
        if let Some(v6) = hostport.strip_prefix('[') { v6.split(']').next()? } else { hostport.split(':').next()? };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// True if the URL points at this machine or a private network. Stops users
/// from making the bot fetch internal services (SSRF) and post the result.
pub fn is_private_url(url: &str) -> bool {
    match host_of(url) {
        Some(h) => is_private_host(&h),
        None => true,
    }
}

fn is_private_host(host: &str) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_private_v4(v4),
            IpAddr::V6(v6) => is_private_v6(v6),
        };
    }
    // Single-label names ("localhost", docker service names) and odd numeric
    // spellings of IPs (0x7f.1, 2130706433) that resolvers still accept.
    if !host.contains('.') {
        return true;
    }
    if host.chars().all(|c| c.is_ascii_hexdigit() || c == '.' || c == 'x') {
        return true;
    }
    [".localhost", ".local", ".internal", ".lan", ".home", ".intranet"].iter().any(|s| host.ends_with(s))
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1])) // CGNAT
        || o[0] >= 224 // multicast + reserved
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_private_v4(v4);
    }
    let seg = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || (seg[0] & 0xfe00) == 0xfc00 // unique local
        || (seg[0] & 0xffc0) == 0xfe80 // link local
        || (seg[0] & 0xff00) == 0xff00 // multicast
}

/// Canonical form used only as a cache key: fragment and tracking params dropped.
pub fn normalize_url(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or(url);
    let Some((base, query)) = no_frag.split_once('?') else {
        return no_frag.to_string();
    };
    const TRACKING: [&str; 18] = [
        "si",
        "feature",
        "igsh",
        "igshid",
        "fbclid",
        "gclid",
        "ref_src",
        "ref_url",
        "pp",
        "ysclid",
        "dnt",
        "origin",
        "source",
        "context",
        "sender_device",
        "is_from_webapp",
        "t",
        "time_continue",
    ];
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| {
            let key = p.split('=').next().unwrap_or("");
            !p.is_empty() && !key.starts_with("utm_") && !TRACKING.contains(&key)
        })
        .collect();
    if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}

pub fn fmt_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn progress_bar(pct: f32) -> String {
    const WIDTH: usize = 10;
    let filled = ((pct.clamp(0.0, 100.0) / 100.0) * WIDTH as f32) as usize;
    let mut s = String::with_capacity(WIDTH * 3);
    for i in 0..WIDTH {
        s.push(if i < filled { '▰' } else { '▱' });
    }
    s
}

/// Cut a string to at most `max` chars (not bytes) on a char boundary.
pub fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Filesystem- and Telegram-safe file stem, capped at `max_bytes`.
pub fn sanitize_filename(name: &str, max_bytes: usize) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_space = false;
    for c in name.chars() {
        let c =
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { ' ' } else { c };
        if c.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    let mut cut = out.len().min(max_bytes);
    while !out.is_char_boundary(cut) {
        cut -= 1;
    }
    out.truncate(cut);
    let out = out.trim_matches([' ', '.']).to_string();
    if out.is_empty() {
        "media".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_urls() {
        let urls = extract_urls("look: https://youtu.be/abc?x=1, and HTTP://Example.com/a.", 3);
        assert_eq!(urls, vec!["https://youtu.be/abc?x=1", "HTTP://Example.com/a"]);
        assert!(extract_urls("no links here http:// https://", 3).is_empty());
        assert_eq!(extract_urls("https://a.com https://a.com https://b.com https://c.com", 2).len(), 2);
        assert_eq!(extract_urls("(https://a.com/x)", 3), vec!["https://a.com/x"]);
        assert_eq!(extract_urls("https://en.wikipedia.org/wiki/A_(b)", 3), vec!["https://en.wikipedia.org/wiki/A_(b)"]);
    }

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://user:pw@WWW.Example.com:8080/p?q#f").as_deref(), Some("www.example.com"));
        assert_eq!(host_of("http://[::1]:80/x").as_deref(), Some("::1"));
    }

    #[test]
    fn private_urls() {
        for u in [
            "http://localhost/x",
            "http://127.0.0.1:8081/",
            "http://10.1.2.3/",
            "http://192.168.0.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://telegram-bot-api:8081/",
            "http://2130706433/",
            "http://0x7f.0.0.1/",
            "http://router.lan/",
            "http://[::ffff:10.0.0.1]/",
        ] {
            assert!(is_private_url(u), "{u} should be private");
        }
        for u in ["https://youtube.com/watch?v=1", "https://8.8.8.8/x", "https://cdn.example.org/a.mp4"] {
            assert!(!is_private_url(u), "{u} should be public");
        }
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize_url("https://youtu.be/abc?si=xyz#t=1"), "https://youtu.be/abc");
        assert_eq!(normalize_url("https://a.com/w?v=1&utm_source=x&feature=share"), "https://a.com/w?v=1");
        assert_eq!(normalize_url("https://a.com/w"), "https://a.com/w");
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_size(5 * 1_048_576 + 524_288), "5.5 MB");
        assert_eq!(fmt_size(150 * 1_048_576), "150 MB");
        assert_eq!(fmt_size(1536 * 1_048_576), "1.50 GB");
        assert_eq!(fmt_duration(59), "0:59");
        assert_eq!(fmt_duration(3723), "1:02:03");
        assert_eq!(progress_bar(0.0), "▱▱▱▱▱▱▱▱▱▱");
        assert_eq!(progress_bar(55.0), "▰▰▰▰▰▱▱▱▱▱");
        assert_eq!(progress_bar(250.0), "▰▰▰▰▰▰▰▰▰▰");
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize_filename("a/b\\c: d?  e", 100), "a b c d e");
        assert_eq!(sanitize_filename("...", 100), "media");
        assert_eq!(sanitize_filename("Привет мир", 9), "Прив");
        assert_eq!(truncate_chars("Привет", 3), "При");
    }
}
