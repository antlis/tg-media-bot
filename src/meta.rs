//! Lightweight metadata & title resolver with rate limiting and circuit breaking.
//!
//! Fetches titles via oEmbed and OpenGraph without invoking yt-dlp,
//! protecting the server's IP and saving system resources.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Tracks consecutive failures and trips to "open" state when errors or 429 occur.
#[derive(Debug)]
pub struct CircuitBreaker {
    consecutive_failures: AtomicU32,
    open_until: AtomicU64,
    failure_threshold: u32,
    cooldown_secs: u64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            open_until: AtomicU64::new(0),
            failure_threshold,
            cooldown_secs,
        }
    }

    pub fn can_request(&self) -> bool {
        let now = crate::state::now_secs();
        let until = self.open_until.load(Ordering::Relaxed);
        now >= until
    }

    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    pub fn record_failure(&self, is_rate_limited: bool) {
        let now = crate::state::now_secs();
        if is_rate_limited {
            // Immediate trip on HTTP 429
            self.open_until.store(now + self.cooldown_secs, Ordering::Relaxed);
            log::warn!("metadata circuit breaker tripped by 429 rate limit; cooling down for {}s", self.cooldown_secs);
            return;
        }

        let fails = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if fails >= self.failure_threshold {
            self.open_until.store(now + self.cooldown_secs, Ordering::Relaxed);
            log::warn!(
                "metadata circuit breaker tripped after {fails} consecutive failures; cooling down for {}s",
                self.cooldown_secs
            );
        }
    }
}

/// Enforces a minimum time interval between outbound metadata requests.
pub struct RateLimiter {
    last_request: Mutex<Option<Instant>>,
    interval: Duration,
}

impl RateLimiter {
    pub fn new(interval: Duration) -> Self {
        Self { last_request: Mutex::new(None), interval }
    }

    pub async fn acquire(&self) {
        let mut last = self.last_request.lock().await;
        let now = Instant::now();
        if let Some(prev) = *last {
            if now < prev + self.interval {
                let wait = (prev + self.interval) - now;
                tokio::time::sleep(wait).await;
            }
        }
        *last = Some(Instant::now());
    }
}

pub struct TitleResolver {
    client: reqwest::Client,
    breaker: CircuitBreaker,
    limiter: RateLimiter,
}

impl Default for TitleResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl TitleResolver {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .connect_timeout(Duration::from_secs(2))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();

        Self {
            client,
            breaker: CircuitBreaker::new(3, 300), // 3 consecutive failures or 429 -> 5 min cooldown
            limiter: RateLimiter::new(Duration::from_millis(1000)), // 1 request / sec
        }
    }

    pub async fn resolve(&self, url: &str) -> Option<String> {
        if !self.breaker.can_request() {
            log::debug!("skipping title resolve for {url}: circuit breaker is open");
            return None;
        }

        let host = crate::util::host_of(url)?;
        if crate::util::is_private_url(url) {
            return None;
        }

        self.limiter.acquire().await;

        let host_lower = host.to_ascii_lowercase();

        let result = if host_lower.contains("youtube.com") || host_lower.contains("youtu.be") {
            self.fetch_youtube_oembed(url).await
        } else if host_lower.contains("tiktok.com") {
            self.fetch_tiktok_oembed(url).await
        } else if host_lower.contains("vimeo.com") {
            self.fetch_vimeo_oembed(url).await
        } else {
            self.fetch_opengraph(url).await
        };

        match result {
            Ok(Some(title)) => {
                let trimmed = title.trim();
                if !trimmed.is_empty() {
                    self.breaker.record_success();
                    Some(trimmed.to_string())
                } else {
                    None
                }
            }
            Ok(None) => None,
            Err(ResolveError::RateLimited) => {
                self.breaker.record_failure(true);
                None
            }
            Err(ResolveError::Other(e)) => {
                log::debug!("failed to resolve title for {url}: {e}");
                self.breaker.record_failure(false);
                None
            }
        }
    }

    async fn fetch_youtube_oembed(&self, url: &str) -> Result<Option<String>, ResolveError> {
        let resp = self
            .client
            .get("https://www.youtube.com/oembed")
            .query(&[("url", url), ("format", "json")])
            .send()
            .await
            .map_err(|e| ResolveError::Other(e.to_string()))?;

        if resp.status().as_u16() == 429 {
            return Err(ResolveError::RateLimited);
        }
        if !resp.status().is_success() {
            return Err(ResolveError::Other(format!("HTTP {}", resp.status())));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| ResolveError::Other(e.to_string()))?;
        Ok(body.get("title").and_then(|v| v.as_str()).map(html_unescape))
    }

    async fn fetch_tiktok_oembed(&self, url: &str) -> Result<Option<String>, ResolveError> {
        let resp = self
            .client
            .get("https://www.tiktok.com/oembed")
            .query(&[("url", url)])
            .send()
            .await
            .map_err(|e| ResolveError::Other(e.to_string()))?;

        if resp.status().as_u16() == 429 {
            return Err(ResolveError::RateLimited);
        }
        if !resp.status().is_success() {
            return Err(ResolveError::Other(format!("HTTP {}", resp.status())));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| ResolveError::Other(e.to_string()))?;
        Ok(body.get("title").and_then(|v| v.as_str()).map(html_unescape))
    }

    async fn fetch_vimeo_oembed(&self, url: &str) -> Result<Option<String>, ResolveError> {
        let resp = self
            .client
            .get("https://vimeo.com/api/oembed.json")
            .query(&[("url", url)])
            .send()
            .await
            .map_err(|e| ResolveError::Other(e.to_string()))?;

        if resp.status().as_u16() == 429 {
            return Err(ResolveError::RateLimited);
        }
        if !resp.status().is_success() {
            return Err(ResolveError::Other(format!("HTTP {}", resp.status())));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| ResolveError::Other(e.to_string()))?;
        Ok(body.get("title").and_then(|v| v.as_str()).map(html_unescape))
    }

    async fn fetch_opengraph(&self, url: &str) -> Result<Option<String>, ResolveError> {
        let resp = self
            .client
            .get(url)
            .header("User-Agent", "facebookexternalhit/1.1 (+http://www.facebook.com/externalhit_uatext.php)")
            .send()
            .await
            .map_err(|e| ResolveError::Other(e.to_string()))?;

        if resp.status().as_u16() == 429 {
            return Err(ResolveError::RateLimited);
        }
        if !resp.status().is_success() {
            return Err(ResolveError::Other(format!("HTTP {}", resp.status())));
        }

        // Read up to 32 KB of HTML to find <meta> tags in the <head>
        let bytes = resp.bytes().await.map_err(|e| ResolveError::Other(e.to_string()))?;
        let slice = if bytes.len() > 32_768 { &bytes[..32_768] } else { &bytes[..] };
        let text = String::from_utf8_lossy(slice);

        Ok(parse_html_title(&text))
    }
}

enum ResolveError {
    RateLimited,
    Other(String),
}

/// Simple, robust HTML parser to extract og:title, twitter:title, or <title>.
pub fn parse_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();

    // 1. Look through <meta ...> tags
    let mut cursor = 0;
    while let Some(meta_start) = lower[cursor..].find("<meta") {
        let tag_start = cursor + meta_start;
        let tag_end = match lower[tag_start..].find('>') {
            Some(end) => tag_start + end,
            None => break,
        };
        let tag = &html[tag_start..=tag_end];
        let tag_lower = &lower[tag_start..=tag_end];
        cursor = tag_end + 1;

        let is_title_meta = tag_lower.contains("og:title") || tag_lower.contains("twitter:title");
        if is_title_meta {
            if let Some(content) = extract_attribute(tag, "content") {
                let unescaped = html_unescape(content.trim());
                if !unescaped.is_empty() {
                    return Some(unescaped);
                }
            }
        }
    }

    // 2. Fallback to <title>...</title>
    if let Some(start) = lower.find("<title") {
        if let Some(gt) = lower[start..].find('>') {
            let content_start = start + gt + 1;
            if let Some(end) = lower[content_start..].find("</title>") {
                let title = &html[content_start..content_start + end];
                let unescaped = html_unescape(title.trim());
                if !unescaped.is_empty() {
                    return Some(unescaped);
                }
            }
        }
    }

    None
}

fn extract_attribute<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let pattern = format!("{attr_name}=");
    let pos = lower.find(&pattern)?;
    let after_eq = tag[pos + pattern.len()..].trim_start();
    let quote = after_eq.chars().next()?;
    if quote == '"' || quote == '\'' {
        let content = &after_eq[1..];
        let end = content.find(quote)?;
        Some(&content[..end])
    } else {
        let end = after_eq.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(after_eq.len());
        Some(&after_eq[..end])
    }
}

pub fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#039;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_trips_on_failures() {
        let breaker = CircuitBreaker::new(3, 300);
        assert!(breaker.can_request());

        breaker.record_failure(false);
        assert!(breaker.can_request());
        breaker.record_failure(false);
        assert!(breaker.can_request());

        // 3rd failure trips the breaker
        breaker.record_failure(false);
        assert!(!breaker.can_request());
    }

    #[test]
    fn circuit_breaker_trips_immediately_on_rate_limit() {
        let breaker = CircuitBreaker::new(3, 300);
        assert!(breaker.can_request());

        // HTTP 429 trips immediately
        breaker.record_failure(true);
        assert!(!breaker.can_request());
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let breaker = CircuitBreaker::new(3, 300);
        breaker.record_failure(false);
        breaker.record_failure(false);
        breaker.record_success();

        // One more failure should not trip because count was reset
        breaker.record_failure(false);
        assert!(breaker.can_request());
    }

    #[test]
    fn parses_og_and_html_titles() {
        let html1 = r#"<html><head><meta property="og:title" content="Rick Astley &amp; Friends"></head></html>"#;
        assert_eq!(parse_html_title(html1), Some("Rick Astley & Friends".to_string()));

        let html2 = r#"<meta content="Reverse Order" property="og:title">"#;
        assert_eq!(parse_html_title(html2), Some("Reverse Order".to_string()));

        let html3 = r#"<meta name='twitter:title' content='Twitter Single Quote'>"#;
        assert_eq!(parse_html_title(html3), Some("Twitter Single Quote".to_string()));

        let html4 = r#"<html><head><title>Simple Page Title</title></head></html>"#;
        assert_eq!(parse_html_title(html4), Some("Simple Page Title".to_string()));
    }
}
