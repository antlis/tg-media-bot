//! Custom extractor plugins: executables in `PLUGIN_DIR` that turn a page URL
//! into a media URL yt-dlp can download.
//!
//! Protocol (language-agnostic — Python, shell, anything with a shebang):
//!   * invoked as `<plugin> <page-url>` with `TGMB_MODE=resolve` (before
//!     yt-dlp) or `TGMB_MODE=fallback` (after every extractor gave up);
//!   * a plugin that does not handle the URL exits non-zero or prints nothing;
//!   * otherwise it prints either a media URL (optionally followed by a
//!     Referer on the second line) or a JSON object
//!     `{"url": "...", "referer": "...", "headers": {"Origin": "..."}}`.
//!
//! Plugins run one at a time, at reduced priority, with a timeout. Unlike the
//! original bot there is no bundled headless Chromium: a browser-based
//! fallback can still be plugged in here when it is really needed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;
use crate::proc;
use crate::ytdlp::NetOpts;

const RESOLVE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub url: String,
    pub referer: Option<String>,
    pub headers: Vec<(String, String)>,
}

impl Resolved {
    pub fn net_opts(&self) -> NetOpts {
        NetOpts {
            cookies: false,
            proxy: false,
            force_ipv4: false,
            referer: self.referer.clone(),
            headers: self.headers.clone(),
            file_urls: self.url.starts_with("file://"),
        }
    }
}

pub struct Plugins {
    list: Vec<PathBuf>,
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

impl Plugins {
    pub fn none() -> Plugins {
        Plugins { list: Vec::new() }
    }

    pub fn load(dir: Option<&Path>) -> Plugins {
        let Some(dir) = dir else { return Plugins::none() };
        let Ok(entries) = std::fs::read_dir(dir) else {
            log::warn!("PLUGIN_DIR {} is not readable; plugins disabled", dir.display());
            return Plugins::none();
        };
        let mut list: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                !name.starts_with(['.', '_']) && !name.ends_with(".md") && !name.ends_with(".txt")
            })
            .filter(|p| {
                let ok = is_executable(p);
                if !ok && p.is_file() {
                    log::warn!("plugin {} is not executable (chmod +x); skipped", p.display());
                }
                ok
            })
            .collect();
        list.sort();
        for p in &list {
            log::info!("loaded plugin {}", p.display());
        }
        Plugins { list }
    }

    /// First plugin that handles `url` wins; failures fall through.
    pub async fn resolve(&self, cfg: &Config, url: &str, fallback: bool) -> Option<Resolved> {
        for plugin in &self.list {
            let mut cmd = proc::command(&plugin.to_string_lossy(), cfg.child_nice);
            cmd.arg(url).env("TGMB_MODE", if fallback { "fallback" } else { "resolve" });
            match proc::capture(cmd, RESOLVE_TIMEOUT).await {
                Ok((out, stdout)) if out.ok => {
                    if let Some(r) = parse_output(&stdout, url) {
                        log::info!("plugin {} resolved the link", plugin.display());
                        return Some(r);
                    }
                }
                Ok(_) => {}
                Err(e) => log::warn!("plugin {} failed: {e:?}", plugin.display()),
            }
        }
        None
    }
}

#[derive(Deserialize)]
struct JsonOut {
    #[serde(alias = "media_url")]
    url: String,
    referer: Option<String>,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
}

fn acceptable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
}

pub fn parse_output(stdout: &str, page_url: &str) -> Option<Resolved> {
    let text = stdout.trim();
    if text.is_empty() {
        return None;
    }
    let resolved = if text.starts_with('{') {
        let j: JsonOut = serde_json::from_str(text).ok()?;
        let headers = j
            .headers
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k, v.to_string())))
            .filter(|(k, v)| !k.contains([':', '\n']) && !v.contains('\n'))
            .collect();
        Resolved { url: j.url, referer: j.referer, headers }
    } else {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        let url = lines.next()?.to_string();
        let referer = lines.next().map(str::to_string);
        Resolved { url, referer, headers: Vec::new() }
    };
    if !acceptable(&resolved.url) {
        return None;
    }
    let referer = resolved.referer.filter(|r| acceptable(r)).or_else(|| Some(page_url.to_string()));
    Some(Resolved { referer, ..resolved })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_output_formats() {
        let page = "https://site.com/watch/1";
        let r = parse_output("https://cdn.site.com/v.m3u8\n", page).unwrap();
        assert_eq!(r.url, "https://cdn.site.com/v.m3u8");
        assert_eq!(r.referer.as_deref(), Some(page));

        let r = parse_output("https://cdn/v.mp4\nhttps://embed.site.com/\n", page).unwrap();
        assert_eq!(r.referer.as_deref(), Some("https://embed.site.com/"));

        let r = parse_output(
            r#"{"media_url": "https://cdn/v.m3u8", "headers": {"Origin": "https://site.com", "Bad:Key": "x", "N": 1}}"#,
            page,
        )
        .unwrap();
        assert_eq!(r.headers, vec![("Origin".to_string(), "https://site.com".to_string())]);
        assert!(r.net_opts().referer.is_some() && !r.net_opts().file_urls);

        assert!(parse_output("file:///data/tmp/x.m3u8", page).unwrap().net_opts().file_urls);
        assert!(parse_output("", page).is_none());
        assert!(parse_output("not a url", page).is_none());
        assert!(parse_output("{broken", page).is_none());
    }
}
