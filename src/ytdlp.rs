//! yt-dlp driver.
//!
//! Each download is two cheap steps instead of one blind one:
//!   1. *probe*  — `yt-dlp --dump-json` (metadata only). Lets us refuse live
//!      streams, over-long and over-size media before a single byte is
//!      downloaded, and pick the best quality that fits the upload limit.
//!   2. *download* — `yt-dlp --load-info-json`, reusing the probe result so the
//!      page is not extracted twice.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Deserializer};
use serde_json::Value;
use tokio::sync::watch;

use crate::cachekey;
use crate::config::{AudioFormat, Config};
use crate::plugins::Plugins;
use crate::proc::{self, RunError};
use crate::state::CacheEntry;
use crate::types::{Kind, Stage};

const PROBE_TIMEOUT: Duration = Duration::from_secs(120);
const PROGRESS_PREFIX: &str = "PROG|";
const PROGRESS_TEMPLATE: &str =
    "download:PROG|%(progress._percent_str)s|%(progress.speed)s|%(progress.eta)s|%(progress.downloaded_bytes)s";
/// Lower quality caps tried when a site's fragment URLs expire mid-download.
const EXPIRY_LADDER: [u32; 3] = [720, 480, 360];
const MAX_ATTEMPTS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum Reject {
    Live,
    TooLong { secs: u64 },
    TooLarge { bytes: u64 },
    NoMedia,
}

#[derive(Debug)]
pub enum DlError {
    /// Refused up front (or aborted) by our own limits.
    Rejected(Reject),
    /// yt-dlp failed; carries its last error line.
    Failed(String),
    ProbeTimeout,
    DownloadTimeout,
    Internal(String),
}

/// Known failure classes that get a friendly, translated explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    Age,
    Bot,
    Private,
    Members,
    Login,
    Format,
    Unavailable,
    Unsupported,
    NoMedia,
    NotFound,
    Forbidden,
    RateLimited,
    Network,
    Geo,
    Drm,
    Live,
}

const GEO_MARKERS: [&str; 7] = [
    "geo restriction",
    "geo-restricted",
    "not available from your location",
    "not available in your country",
    "available in your country",
    "in your region",
    "blocked it in your country",
];

const CONNECTIVITY_MARKERS: [&str; 10] = [
    "handshake operation timed out",
    "connection refused",
    "network is unreachable",
    "unable to download webpage",
    "unable to connect to proxy",
    "connection reset by peer",
    "name or service not known",
    "temporary failure in name resolution",
    "nodename nor servname provided",
    "read timed out",
];

fn has_any(e: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| e.contains(m))
}

fn is_geo(e: &str) -> bool {
    has_any(e, &GEO_MARKERS)
}

fn is_connectivity(e: &str) -> bool {
    // "Unable to download webpage: HTTP Error 403" is an answer, not a dropped
    // connection: retrying the same request won't change it.
    has_any(e, &CONNECTIVITY_MARKERS) && !e.contains("http error 4")
}

/// A format/extraction problem that stale or flagged cookies can cause.
fn is_format_error(e: &str) -> bool {
    has_any(e, &["requested format is not available", "unable to extract", "no video formats"])
}

/// Fragment URLs expired before the download could finish.
fn is_fragment_expiry(e: &str) -> bool {
    has_any(e, &["downloaded file is empty", "http error 410"])
}

/// No extractor could handle the page at all.
fn is_unsupported(e: &str) -> bool {
    has_any(e, &["unsupported url", "unable to extract", "no video formats found", "no suitable"])
}

/// Map a raw yt-dlp error line to a user-facing hint, if it is a known one.
pub fn classify(raw: &str) -> Option<Hint> {
    let e = raw.to_lowercase();
    let e = e.as_str();
    let table: [(&[&str], Hint); 15] = [
        (&["confirm your age", "age-restricted", "age restricted"], Hint::Age),
        (&["not a bot"], Hint::Bot),
        (&["drm protected", "drm-protected", "drm protection", "has drm"], Hint::Drm),
        (&["private video", "video is private", "is private"], Hint::Private),
        (&["members-only", "members only", "join this channel"], Hint::Members),
        (&GEO_MARKERS, Hint::Geo),
        (&["live event will begin", "premieres in", "is not currently live"], Hint::Live),
        (&["login required", "only available for registered", "--cookies", "log in", "sign in"], Hint::Login),
        (&["requested format is not available"], Hint::Format),
        (
            &["video unavailable", "video is unavailable", "has been removed", "no longer available", "does not exist"],
            Hint::Unavailable,
        ),
        (&["no video formats found", "no media found", "there is no video", "no video could be found"], Hint::NoMedia),
        (&["unsupported url", "unable to extract", "no suitable"], Hint::Unsupported),
        (&["http error 404"], Hint::NotFound),
        (&["http error 429", "too many requests"], Hint::RateLimited),
        (&["http error 403"], Hint::Forbidden),
    ];
    for (markers, hint) in table {
        if has_any(e, markers) {
            return Some(hint);
        }
    }
    if is_connectivity(e) || e.contains("timed out") {
        return Some(Hint::Network);
    }
    None
}

// ---------------------------------------------------------------------------
// Probe result
// ---------------------------------------------------------------------------

fn num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

fn text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) if !s.is_empty() => Some(s),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn flag<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(matches!(Value::deserialize(d)?, Value::Bool(true)))
}

/// One entry of yt-dlp's `formats`. Everything is optional and leniently
/// typed: extractors are inconsistent and a surprise must not fail the probe.
#[derive(Deserialize, Default, Debug, Clone)]
pub struct Format {
    #[serde(default, deserialize_with = "text")]
    pub ext: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub vcodec: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub acodec: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub resolution: Option<String>,
    #[serde(default, deserialize_with = "num")]
    pub height: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub tbr: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub filesize: Option<f64>,
    #[serde(default, deserialize_with = "num")]
    pub filesize_approx: Option<f64>,
    #[serde(default, deserialize_with = "flag")]
    pub has_drm: bool,
}

impl Format {
    fn codec_is(codec: &Option<String>, value: &str) -> bool {
        codec.as_deref() == Some(value)
    }

    fn is_storyboard(&self) -> bool {
        self.ext.as_deref() == Some("mhtml")
            || (Self::codec_is(&self.vcodec, "none") && Self::codec_is(&self.acodec, "none"))
    }

    pub fn is_video(&self) -> bool {
        !self.is_storyboard()
            && !self.has_drm
            && !Self::codec_is(&self.vcodec, "none")
            && self.resolution.as_deref() != Some("audio only")
    }

    pub fn is_audio_only(&self) -> bool {
        !self.is_storyboard() && !self.has_drm && !self.is_video() && !Self::codec_is(&self.acodec, "none")
    }

    fn is_video_only(&self) -> bool {
        self.is_video() && Self::codec_is(&self.acodec, "none")
    }

    fn is_h264(&self) -> bool {
        self.vcodec.as_deref().is_some_and(|v| v.starts_with("avc") || v.starts_with("h264"))
    }

    fn height(&self) -> Option<u32> {
        self.height.filter(|h| *h >= 1.0).map(|h| h as u32)
    }

    /// Size in bytes: exact, approximate, or derived from the bitrate.
    fn est_bytes(&self, duration: f64) -> Option<u64> {
        let from_bitrate = self.tbr.filter(|t| *t > 0.0 && duration > 0.0).map(|t| t * 125.0 * duration);
        self.filesize
            .filter(|s| *s > 0.0)
            .or(self.filesize_approx.filter(|s| *s > 0.0))
            .or(from_bitrate)
            .map(|s| s as u64)
    }
}

#[derive(Deserialize, Default, Debug)]
pub struct Info {
    #[serde(default, deserialize_with = "text")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub extractor_key: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub track: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub artist: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub uploader: Option<String>,
    #[serde(default, deserialize_with = "num")]
    pub duration: Option<f64>,
    #[serde(default, deserialize_with = "flag")]
    pub is_live: bool,
    #[serde(default, deserialize_with = "text")]
    pub live_status: Option<String>,
    #[serde(default)]
    pub formats: Vec<Format>,
}

impl Info {
    fn duration(&self) -> f64 {
        self.duration.unwrap_or(0.0)
    }

    /// True when the source has audio but no video at all (e.g. SoundCloud).
    pub fn audio_only(&self) -> bool {
        self.formats.iter().any(Format::is_audio_only) && !self.formats.iter().any(Format::is_video)
    }

    /// Size of the best audio-only stream, used when it has to be merged in.
    fn audio_est(&self) -> Option<u64> {
        self.formats
            .iter()
            .filter(|f| f.is_audio_only())
            .max_by(|a, b| a.tbr.unwrap_or(0.0).total_cmp(&b.tbr.unwrap_or(0.0)))
            .and_then(|f| f.est_bytes(self.duration()))
    }

    /// Estimated download size at exactly `height`, preferring H.264.
    fn est_for_height(&self, height: u32) -> Option<u64> {
        let at: Vec<&Format> = self.formats.iter().filter(|f| f.is_video() && f.height() == Some(height)).collect();
        let h264: Vec<&Format> = at.iter().copied().filter(|f| f.is_h264()).collect();
        let pool = if h264.is_empty() { at } else { h264 };
        let best = pool.into_iter().max_by(|a, b| a.tbr.unwrap_or(0.0).total_cmp(&b.tbr.unwrap_or(0.0)))?;
        let video = best.est_bytes(self.duration())?;
        if best.is_video_only() {
            Some(video + self.audio_est().unwrap_or(0))
        } else {
            Some(video)
        }
    }

    /// Distinct video heights, highest first.
    fn heights(&self) -> Vec<u32> {
        let mut hs: Vec<u32> = self.formats.iter().filter(|f| f.is_video()).filter_map(Format::height).collect();
        hs.sort_unstable_by(|a, b| b.cmp(a));
        hs.dedup();
        hs
    }
}

/// Refuse things that would hurt a small server before downloading anything.
pub fn check(info: &Info, cfg: &Config) -> Result<(), Reject> {
    let live = info.is_live || matches!(info.live_status.as_deref(), Some("is_live") | Some("is_upcoming"));
    if live {
        return Err(Reject::Live);
    }
    let secs = info.duration() as u64;
    if cfg.max_duration > 0 && secs > cfg.max_duration {
        return Err(Reject::TooLong { secs });
    }
    Ok(())
}

/// What to ask yt-dlp for.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub selector: String,
    pub sort: Option<&'static str>,
    pub audio: bool,
    /// Chosen height cap, if the source advertises heights.
    pub height: Option<u32>,
    /// Estimated size in bytes, if known.
    pub est: Option<u64>,
}

/// Pick the best quality that is at most `cap` pixels high and fits `limit`
/// bytes. `keep_codec` (audio): prefer AAC/MP3 sources that need no re-encode.
pub fn plan(info: &Info, kind: Kind, cap: u32, limit: u64, keep_codec: bool) -> Result<Plan, Reject> {
    if kind == Kind::Audio {
        let est = info.audio_est();
        if let Some(bytes) = est.filter(|e| *e > limit) {
            return Err(Reject::TooLarge { bytes });
        }
        let selector =
            if keep_codec { "ba[acodec^=mp4a]/ba[acodec=mp3]/ba/b[height<=?480]/b" } else { "ba/b[height<=?480]/b" };
        return Ok(Plan { selector: selector.to_string(), sort: None, audio: true, height: None, est });
    }

    let heights = info.heights();
    if heights.is_empty() {
        // No height info (direct file links, odd extractors): take what there
        // is, refusing only if even the smallest known variant is too big.
        let duration = info.duration();
        let known: Vec<u64> =
            info.formats.iter().filter(|f| f.is_video()).filter_map(|f| f.est_bytes(duration)).collect();
        let smallest = known.iter().copied().min();
        if let Some(bytes) = smallest.filter(|s| *s > limit) {
            return Err(Reject::TooLarge { bytes });
        }
        let est = if known.len() == 1 { smallest } else { None };
        return Ok(Plan { selector: "bv*+ba/b".to_string(), sort: None, audio: false, height: None, est });
    }

    let mut candidates: Vec<u32> = heights.iter().copied().filter(|h| *h <= cap).collect();
    if candidates.is_empty() {
        // Everything is above the cap: the smallest variant is the best we can do.
        candidates.push(*heights.last().expect("non-empty"));
    }
    let mut too_big = 0;
    for height in candidates {
        let est = info.est_for_height(height);
        if let Some(bytes) = est.filter(|e| *e > limit) {
            too_big = bytes;
            continue;
        }
        // H.264 plays inline everywhere and needs no re-encode. Prefer it as
        // long as that does not cost more than a drop to 720p.
        let h264_ok = info
            .formats
            .iter()
            .filter(|f| f.is_video() && f.is_h264())
            .filter_map(Format::height)
            .any(|h| h <= height && h >= height.min(720));
        let sort = if h264_ok { "vcodec:h264,res,fps,acodec:aac" } else { "res,fps,vcodec:h264,acodec:aac" };
        return Ok(Plan {
            selector: format!("bv*[height<=?{height}]+ba/b[height<=?{height}]/bv*+ba/b"),
            sort: Some(sort),
            audio: false,
            height: Some(height),
            est,
        });
    }
    Err(Reject::TooLarge { bytes: too_big })
}

// ---------------------------------------------------------------------------
// Running yt-dlp
// ---------------------------------------------------------------------------

/// Per-attempt network identity.
#[derive(Debug, Clone, Default)]
pub struct NetOpts {
    pub cookies: bool,
    pub proxy: bool,
    pub force_ipv4: bool,
    pub referer: Option<String>,
    pub headers: Vec<(String, String)>,
    pub file_urls: bool,
}

fn base_args(cfg: &Config, net: &NetOpts) -> Vec<String> {
    let mut a: Vec<String> = [
        "--ignore-config",
        "--no-playlist",
        "--playlist-items",
        "1",
        "--no-warnings",
        "--color",
        "never",
        "--socket-timeout",
        "30",
        "--retries",
        "3",
        "--fragment-retries",
        "5",
        "--extractor-retries",
        "2",
        "--no-write-comments",
        "--no-write-subs",
        "--no-write-playlist-metafiles",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    if net.force_ipv4 && !a.iter().any(|x| x == "--force-ipv4" || x == "-4") {
        a.push("--force-ipv4".to_string());
    }

    if net.proxy {
        if let Some(proxy) = &cfg.proxy_url {
            a.extend(["--proxy".to_string(), proxy.clone()]);
        }
    }
    if net.cookies {
        if let Some(file) = cfg.cookies_path() {
            a.extend(["--cookies".to_string(), file.display().to_string()]);
        } else if let Some(browser) = &cfg.browser_cookies {
            a.extend(["--cookies-from-browser".to_string(), browser.clone()]);
        }
    }
    if let Some(referer) = &net.referer {
        a.extend(["--referer".to_string(), referer.clone()]);
    }
    for (key, value) in &net.headers {
        a.extend(["--add-header".to_string(), format!("{key}:{value}")]);
    }
    if net.file_urls {
        a.push("--enable-file-urls".to_string());
    }
    a.extend(cfg.ytdlp_extra_args.iter().cloned());
    a
}

/// `%` is special in yt-dlp output templates.
fn template_path(dir: &Path, name: &str) -> String {
    format!("{}", dir.join(name).display()).replace('%', "%%").replace("%%(ext)s", "%(ext)s")
}

fn download_args(cfg: &Config, net: &NetOpts, plan: &Plan, dir: &Path) -> Vec<String> {
    let mut a = base_args(cfg, net);
    let s = |v: &str| v.to_string();
    a.extend([s("--load-info-json"), dir.join("info.json").display().to_string(), s("-f"), plan.selector.clone()]);
    if let Some(sort) = plan.sort {
        a.extend([s("-S"), s(sort)]);
    }
    a.extend([
        s("-o"),
        template_path(dir, "media.%(ext)s"),
        s("-o"),
        format!("thumbnail:{}", template_path(dir, "cover.%(ext)s")),
        s("--write-thumbnail"),
        s("--no-mtime"),
        s("--no-part"),
        s("--buffer-size"),
        s("1024k"),
        s("--concurrent-fragments"),
        cfg.concurrent_fragments.to_string(),
    ]);
    if let Some(rate) = &cfg.rate_limit {
        a.extend([s("--limit-rate"), rate.clone()]);
    }
    if plan.audio {
        // "auto": containers that already hold AAC/MP3 are remuxed (stream
        // copy, no CPU); anything else is encoded to MP3.
        let format = match cfg.audio_format {
            AudioFormat::Auto => "m4a>m4a/mp3>mp3/mp4>m4a/mov>m4a/aac>m4a/mp3",
            AudioFormat::Mp3 => "mp3",
        };
        a.extend([
            s("-x"),
            s("--audio-format"),
            s(format),
            s("--audio-quality"),
            cfg.audio_quality.clone(),
            s("--embed-thumbnail"),
            s("--embed-metadata"),
        ]);
    } else {
        // Merging is a remux, not a re-encode. `faststart` moves the index to
        // the front so Telegram can stream the file.
        a.extend([s("--merge-output-format"), s("mp4"), s("--postprocessor-args"), s("Merger:-movflags +faststart")]);
    }
    a.extend([s("--progress"), s("--newline"), s("--progress-template"), s(PROGRESS_TEMPLATE)]);
    a
}

#[derive(Debug, PartialEq)]
struct Progress {
    pct: Option<f32>,
    speed: Option<f64>,
    eta: Option<u64>,
    downloaded: Option<u64>,
}

fn parse_progress(line: &str) -> Option<Progress> {
    let rest = line.trim_start().strip_prefix(PROGRESS_PREFIX)?;
    let mut parts = rest.split('|').map(str::trim);
    let pct = parts.next().and_then(|p| p.trim_end_matches('%').trim().parse::<f32>().ok());
    let speed = parts.next().and_then(|p| p.parse::<f64>().ok()).filter(|s| *s > 0.0);
    let eta = parts.next().and_then(|p| p.parse::<f64>().ok()).map(|e| e.max(0.0) as u64);
    let downloaded = parts.next().and_then(|p| p.parse::<f64>().ok()).map(|d| d.max(0.0) as u64);
    Some(Progress { pct, speed, eta, downloaded })
}

/// yt-dlp announces post-processing steps as "[Merger] ...", "[ExtractAudio] ...".
fn is_postprocess_line(line: &str) -> bool {
    const MARKERS: [&str; 9] = [
        "[Merger]",
        "[ExtractAudio]",
        "[VideoConvertor]",
        "[VideoRemuxer]",
        "[EmbedThumbnail]",
        "[Metadata]",
        "[ThumbnailsConvertor]",
        "[FixupM3u8]",
        "[FixupM4a]",
    ];
    MARKERS.iter().any(|m| line.starts_with(m))
}

/// The most useful line of a failed run's stderr.
fn error_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.contains("ERROR:"))
        .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("yt-dlp exited with an error");
    let line = line.split_once("ERROR:").map(|(_, rest)| rest).unwrap_or(line).trim();
    crate::util::truncate_chars(line, 400).to_string()
}

/// Step 1: metadata only. Writes `info.json` into `dir` and parses it.
pub async fn probe(cfg: &Config, url: &str, net: &NetOpts, dir: &Path) -> Result<Info, DlError> {
    let info_path = dir.join("info.json");
    let file =
        std::fs::File::create(&info_path).map_err(|e| DlError::Internal(format!("{}: {e}", info_path.display())))?;
    let mut cmd = proc::command(&cfg.ytdlp_bin, cfg.child_nice);
    cmd.args(base_args(cfg, net)).arg("--dump-json").arg(url);
    cmd.stdout(Stdio::from(file)).stderr(Stdio::piped());
    let out = match proc::run(cmd, PROBE_TIMEOUT, |_| ControlFlow::Continue(())).await {
        Ok(out) => out,
        Err(RunError::Timeout) => return Err(DlError::ProbeTimeout),
        Err(RunError::Stopped) => return Err(DlError::Internal("process stopped".into())),
        Err(RunError::Spawn(e)) => return Err(DlError::Internal(format!("could not run yt-dlp: {e}"))),
    };
    if !out.ok {
        return Err(DlError::Failed(error_line(&out.stderr)));
    }
    let bytes = tokio::fs::read(&info_path).await.map_err(|e| DlError::Internal(e.to_string()))?;
    let first = bytes.split(|b| *b == b'\n').find(|l| !l.iter().all(u8::is_ascii_whitespace));
    let Some(first) = first else {
        return Err(DlError::Rejected(Reject::NoMedia));
    };
    serde_json::from_slice::<Info>(first).map_err(|e| DlError::Internal(format!("unexpected yt-dlp output: {e}")))
}

/// Step 2: fetch the planned format(s) into `dir`.
async fn download(
    cfg: &Config,
    net: &NetOpts,
    plan: &Plan,
    dir: &Path,
    stage: &watch::Sender<Stage>,
) -> Result<(), DlError> {
    let mut cmd = proc::command(&cfg.ytdlp_bin, cfg.child_nice);
    cmd.args(download_args(cfg, net, plan, dir));
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Sizes are not always known up front; never let one file run past the cap.
    let hard_cap = cfg.max_file_bytes + cfg.max_file_bytes / 10;
    let mut overflow = 0u64;
    let result = proc::run(cmd, Duration::from_secs(cfg.download_timeout), |line| {
        if let Some(p) = parse_progress(line) {
            if let Some(done) = p.downloaded.filter(|d| *d > hard_cap) {
                overflow = done;
                return ControlFlow::Break(());
            }
            stage.send_replace(Stage::Downloading { pct: p.pct, speed: p.speed, eta: p.eta });
        } else if is_postprocess_line(line) {
            stage.send_replace(Stage::Processing);
        }
        ControlFlow::Continue(())
    })
    .await;

    match result {
        Ok(out) if out.ok => Ok(()),
        Ok(out) => Err(DlError::Failed(error_line(&out.stderr))),
        Err(RunError::Stopped) => Err(DlError::Rejected(Reject::TooLarge { bytes: overflow })),
        Err(RunError::Timeout) => Err(DlError::DownloadTimeout),
        Err(RunError::Spawn(e)) => Err(DlError::Internal(format!("could not run yt-dlp: {e}"))),
    }
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

const IMAGE_EXTS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "avif"];

/// The downloaded media file: the largest `media.*` that is not a leftover.
fn find_media(dir: &Path) -> Option<PathBuf> {
    const SKIP: [&str; 5] = ["part", "ytdl", "json", "temp", "tmp"];
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("media.")))
        .filter(|p| {
            let ext = extension(p);
            !SKIP.contains(&ext.as_str()) && !IMAGE_EXTS.contains(&ext.as_str())
        })
        .filter_map(|p| p.metadata().ok().filter(|m| m.is_file() && m.len() > 0).map(|m| (m.len(), p)))
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

fn find_thumb(dir: &Path) -> Option<PathBuf> {
    let mut thumbs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("cover.")))
        .filter(|p| IMAGE_EXTS.contains(&extension(p).as_str()))
        .collect();
    thumbs.sort_by_key(|p| (extension(p) != "jpg", p.clone()));
    thumbs.into_iter().next()
}

fn clear_dir(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// What the probe decided, before anything is downloaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Probed {
    /// Video identity from metadata (`youtube:<id>`), if the site names one.
    pub video_key: Option<String>,
    /// May differ from the request: audio-only sources are always audio.
    pub kind: Kind,
    /// Height that will be downloaded, if the source advertises heights.
    pub height: Option<u32>,
}

pub struct Fetched {
    pub path: PathBuf,
    pub thumb: Option<PathBuf>,
    pub title: String,
    pub performer: String,
    pub duration: f64,
    pub probed: Probed,
}

pub enum Outcome {
    Downloaded(Fetched),
    /// The probe identified something already uploaded: nothing was downloaded.
    Cached(CacheEntry, Probed),
}

/// Checks the file_id cache once the probe knows what would be downloaded.
pub type Lookup<'a> = &'a (dyn Fn(&Probed) -> Option<CacheEntry> + Sync);

#[allow(clippy::too_many_arguments)]
async fn attempt(
    cfg: &Config,
    target: &str,
    net: &NetOpts,
    kind: Kind,
    cap: u32,
    dir: &Path,
    stage: &watch::Sender<Stage>,
    lookup: Lookup<'_>,
    on_title: &(dyn Fn(&str) + Send + Sync),
) -> Result<Outcome, DlError> {
    stage.send_replace(Stage::Probing);
    let info = probe(cfg, target, net, dir).await?;
    let title = info.track.as_deref().or(info.title.as_deref()).unwrap_or("");
    if !title.is_empty() {
        on_title(title);
    }
    check(&info, cfg).map_err(DlError::Rejected)?;
    let kind = if info.audio_only() { Kind::Audio } else { kind };
    let plan =
        plan(&info, kind, cap, cfg.max_file_bytes, cfg.audio_format == AudioFormat::Auto).map_err(DlError::Rejected)?;
    log::debug!("plan: {plan:?}");
    let probed = Probed {
        video_key: cachekey::meta_video_key(info.extractor_key.as_deref(), info.id.as_deref()),
        kind,
        height: plan.height,
    };
    if let Some(entry) = lookup(&probed) {
        if !entry.title.is_empty() {
            on_title(&entry.title);
        }
        return Ok(Outcome::Cached(entry, probed));
    }

    stage.send_replace(Stage::Downloading { pct: None, speed: None, eta: None });
    download(cfg, net, &plan, dir, stage).await?;

    let path = find_media(dir).ok_or_else(|| DlError::Failed("no media file was produced".into()))?;
    Ok(Outcome::Downloaded(Fetched {
        path,
        thumb: find_thumb(dir),
        title: info.track.or(info.title).unwrap_or_default(),
        performer: info.artist.or(info.uploader).unwrap_or_default(),
        duration: info.duration.unwrap_or(0.0),
        probed,
    }))
}

/// Probe + download with the fallback chain: lower quality on fragment
/// expiry, drop cookies on format errors, retry transient network errors,
/// retry through the proxy on geo/connectivity blocks, and finally ask a
/// fallback plugin when no extractor understands the page.
#[allow(clippy::too_many_arguments)]
pub async fn fetch(
    cfg: &Config,
    plugins: &Plugins,
    url: &str,
    kind: Kind,
    cap: Option<u32>,
    dir: &Path,
    stage: &watch::Sender<Stage>,
    lookup: Lookup<'_>,
    on_title: &(dyn Fn(&str) + Send + Sync),
) -> Result<Outcome, DlError> {
    let mut target = url.to_string();
    let mut net = NetOpts { cookies: cfg.has_cookies(), ..NetOpts::default() };
    if let Some(resolved) = plugins.resolve(cfg, url, false).await {
        net = resolved.net_opts();
        target = resolved.url;
    }

    let mut cap = cap.unwrap_or(cfg.max_height).min(cfg.max_height);
    let mut dropped_cookies = false;
    let mut net_retries = 0;
    let mut tried_fallback = false;

    for _ in 0..MAX_ATTEMPTS {
        clear_dir(dir);
        let raw = match attempt(cfg, &target, &net, kind, cap, dir, stage, lookup, on_title).await {
            Ok(outcome) => return Ok(outcome),
            Err(DlError::Failed(raw)) => raw,
            Err(DlError::ProbeTimeout) => {
                if !net.force_ipv4 {
                    log::info!("probe timed out — retrying with --force-ipv4");
                    net.force_ipv4 = true;
                    continue;
                }
                if !net.proxy && cfg.proxy_url.is_some() {
                    log::info!("probe timed out — retrying through the proxy");
                    net.proxy = true;
                    net.cookies = cfg.has_cookies() && net.referer.is_none();
                    dropped_cookies = false;
                    continue;
                }
                return Err(DlError::ProbeTimeout);
            }
            Err(other) => return Err(other),
        };
        log::info!("yt-dlp failed: {raw}");
        let e = raw.to_lowercase();

        if !net.force_ipv4 && is_connectivity(&e) {
            log::info!("connectivity error — retrying with --force-ipv4");
            net.force_ipv4 = true;
            continue;
        }

        if is_fragment_expiry(&e) {
            if let Some(lower) = EXPIRY_LADDER.into_iter().find(|h| *h < cap) {
                log::info!("fragments expired — retrying at {lower}p");
                cap = lower;
                continue;
            }
        }
        if net.cookies && !dropped_cookies && is_format_error(&e) {
            log::info!("format error with cookies — retrying without cookies");
            net.cookies = false;
            dropped_cookies = true;
            continue;
        }
        if is_connectivity(&e) && net_retries < 2 {
            net_retries += 1;
            log::info!("transient network error — retry {net_retries}/2");
            tokio::time::sleep(Duration::from_secs(2 * net_retries)).await;
            continue;
        }
        if !net.proxy && cfg.proxy_url.is_some() && (is_geo(&e) || is_connectivity(&e)) {
            log::info!("geo/connectivity block — retrying through the proxy");
            net.proxy = true;
            net.cookies = cfg.has_cookies() && net.referer.is_none();
            dropped_cookies = false;
            continue;
        }
        if !tried_fallback && is_unsupported(&e) {
            tried_fallback = true;
            if let Some(resolved) = plugins.resolve(cfg, url, true).await {
                log::info!("fallback plugin resolved a media URL");
                let proxy = net.proxy;
                net = resolved.net_opts();
                net.proxy = proxy;
                target = resolved.url;
                continue;
            }
        }
        return Err(DlError::Failed(raw));
    }
    Err(DlError::Failed("too many failed attempts".into()))
}

/// `yt-dlp --version`, or None if it cannot be run.
pub async fn version(cfg: &Config) -> Option<String> {
    let mut cmd = proc::command(&cfg.ytdlp_bin, 0);
    cmd.arg("--version");
    let (out, text) = proc::capture(cmd, Duration::from_secs(60)).await.ok()?;
    out.ok.then(|| text.trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Env;

    const MB: u64 = 1_048_576;

    fn cfg() -> Config {
        Config::from_env(&Env::from_pairs(&[("BOT_TOKEN", "1:x")])).unwrap()
    }

    fn info(json: &str) -> Info {
        serde_json::from_str(json).unwrap()
    }

    fn youtube_like() -> Info {
        info(
            r#"{
            "title": "Clip", "uploader": "Chan", "duration": 100,
            "formats": [
              {"format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none"},
              {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "tbr": 128, "filesize": 1600000},
              {"format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "tbr": 140, "filesize": 1700000},
              {"format_id": "18", "ext": "mp4", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "height": 360, "tbr": 500, "filesize": 6000000},
              {"format_id": "136", "ext": "mp4", "vcodec": "avc1.4d401f", "acodec": "none", "height": 720, "tbr": 1500, "filesize": 20000000},
              {"format_id": "247", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 720, "tbr": 1200, "filesize": 15000000},
              {"format_id": "137", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "tbr": 4000, "filesize": 60000000},
              {"format_id": "313", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 2160, "tbr": 16000, "filesize": 200000000}
            ]}"#,
        )
    }

    #[test]
    fn lenient_parsing() {
        let i = info(
            r#"{"title": 5, "duration": "12.5", "is_live": null, "formats": [{"height": "720", "tbr": null, "vcodec": null}]}"#,
        );
        assert_eq!(i.title.as_deref(), Some("5"));
        assert_eq!(i.duration, Some(12.5));
        assert!(!i.is_live);
        assert_eq!(i.formats[0].height(), Some(720));
        assert!(i.formats[0].is_video());
        assert!(info("{}").formats.is_empty());
    }

    #[test]
    fn picks_best_quality_that_fits() {
        let i = youtube_like();
        // 1080p (60 MB + audio) does not fit 50 MB → 720p H.264.
        let p = plan(&i, Kind::Video, 1080, 50 * MB, true).unwrap();
        assert_eq!(p.height, Some(720));
        assert_eq!(p.selector, "bv*[height<=?720]+ba/b[height<=?720]/bv*+ba/b");
        assert_eq!(p.sort, Some("vcodec:h264,res,fps,acodec:aac"));
        assert_eq!(p.est, Some(20_000_000 + 1_700_000));

        // With a 2 GB limit the cap decides; 2160p is never chosen.
        let p = plan(&i, Kind::Video, 1080, 2000 * MB, true).unwrap();
        assert_eq!(p.height, Some(1080));

        // A user-picked cap is honoured.
        assert_eq!(plan(&i, Kind::Video, 480, 2000 * MB, true).unwrap().height, Some(360));

        // Nothing fits → refused with the smallest size seen.
        assert_eq!(plan(&i, Kind::Video, 1080, 2 * MB, true), Err(Reject::TooLarge { bytes: 6_000_000 }));
    }

    #[test]
    fn prefers_resolution_when_h264_is_too_small() {
        let i = info(
            r#"{"duration": 60, "formats": [
              {"vcodec": "avc1", "acodec": "mp4a", "height": 360, "tbr": 400},
              {"vcodec": "vp9", "acodec": "none", "height": 1080, "tbr": 2000},
              {"vcodec": "none", "acodec": "opus", "tbr": 128}
            ]}"#,
        );
        let p = plan(&i, Kind::Video, 1080, 2000 * MB, true).unwrap();
        assert_eq!(p.height, Some(1080));
        assert_eq!(p.sort, Some("res,fps,vcodec:h264,acodec:aac"));
        // tbr-based estimate: (2000 + 128) kbit/s * 60 s
        assert_eq!(p.est, Some((2000 + 128) * 125 * 60));
    }

    #[test]
    fn audio_and_unknown_heights() {
        let i = youtube_like();
        let p = plan(&i, Kind::Audio, 1080, 50 * MB, true).unwrap();
        assert!(p.audio);
        assert!(p.selector.starts_with("ba[acodec^=mp4a]/"));
        assert_eq!(plan(&i, Kind::Audio, 1080, 50 * MB, false).unwrap().selector, "ba/b[height<=?480]/b");
        assert_eq!(plan(&i, Kind::Audio, 1080, MB, true), Err(Reject::TooLarge { bytes: 1_700_000 }));

        let direct = info(r#"{"formats": [{"ext": "webm", "filesize": 3000000}]}"#);
        let p = plan(&direct, Kind::Video, 1080, 50 * MB, true).unwrap();
        assert_eq!((p.selector.as_str(), p.height, p.est), ("bv*+ba/b", None, Some(3_000_000)));
        assert_eq!(plan(&direct, Kind::Video, 1080, MB, true), Err(Reject::TooLarge { bytes: 3_000_000 }));

        let sc = info(r#"{"formats": [{"ext": "mp3", "vcodec": "none", "acodec": "mp3", "tbr": 128}]}"#);
        assert!(sc.audio_only());
        assert!(!i.audio_only());
    }

    #[test]
    fn rejects_live_and_long() {
        let cfg = cfg();
        assert_eq!(check(&info(r#"{"is_live": true}"#), &cfg), Err(Reject::Live));
        assert_eq!(check(&info(r#"{"live_status": "is_upcoming"}"#), &cfg), Err(Reject::Live));
        assert_eq!(check(&info(r#"{"duration": 99999}"#), &cfg), Err(Reject::TooLong { secs: 99999 }));
        assert_eq!(check(&info(r#"{"duration": 600, "live_status": "was_live"}"#), &cfg), Ok(()));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(
            parse_progress("PROG|  42.3%|1048576.0|25|12345"),
            Some(Progress { pct: Some(42.3), speed: Some(1048576.0), eta: Some(25), downloaded: Some(12345) })
        );
        assert_eq!(
            parse_progress("PROG|Unknown %|NA|NA|NA"),
            Some(Progress { pct: None, speed: None, eta: None, downloaded: None })
        );
        assert_eq!(parse_progress("[download] Destination: x"), None);
        assert!(is_postprocess_line("[Merger] Merging formats into \"media.mp4\""));
        assert!(!is_postprocess_line("[download] 100%"));
    }

    #[test]
    fn error_classification() {
        assert_eq!(
            error_line("WARNING: x\nERROR: [youtube] abc: Video unavailable\n"),
            "[youtube] abc: Video unavailable"
        );
        assert_eq!(error_line("something odd\n\n"), "something odd");
        assert_eq!(error_line(""), "yt-dlp exited with an error");

        let cases = [
            ("Sign in to confirm your age. This video may be inappropriate", Hint::Age),
            ("Sign in to confirm you're not a bot. Use --cookies-from-browser", Hint::Bot),
            ("[youtube] x: Private video. Sign in if you've been granted access", Hint::Private),
            ("Requested format is not available. Use --list-formats", Hint::Format),
            ("Unsupported URL: https://example.com", Hint::Unsupported),
            ("unable to download video data: HTTP Error 403: Forbidden", Hint::Forbidden),
            ("HTTP Error 404: Not Found", Hint::NotFound),
            ("The uploader has not made this video available in your country", Hint::Geo),
            ("This video is DRM protected", Hint::Drm),
            ("Unable to download webpage: <urlopen error [Errno -2] Name or service not known>", Hint::Network),
            ("Instagram: login required", Hint::Login),
        ];
        for (raw, hint) in cases {
            assert_eq!(classify(raw), Some(hint), "{raw}");
        }
        assert_eq!(classify("[youtube] aaaaaaaaaaa: This video is unavailable"), Some(Hint::Unavailable));
        assert_eq!(classify("some brand new failure"), None);
        assert!(is_connectivity("unable to download webpage: connection reset by peer"));
        assert!(!is_connectivity("unable to download webpage: http error 403: forbidden"));
        assert!(is_fragment_expiry("the downloaded file is empty"));
        assert!(is_format_error("requested format is not available"));
    }

    #[test]
    fn command_lines() {
        let cfg = cfg();
        let dir = Path::new("/data/tmp/ab12");
        let video = Plan { selector: "bv*+ba/b".into(), sort: Some("res"), audio: false, height: None, est: None };
        let a = download_args(&cfg, &NetOpts::default(), &video, dir);
        let joined = a.join(" ");
        assert!(joined.contains("--no-playlist --playlist-items 1"));
        assert!(joined.contains("--no-part --buffer-size 1024k"));
        assert!(joined.contains("--concurrent-fragments 4"));
        assert!(joined.contains("--merge-output-format mp4"));
        assert!(joined.contains("-S res"));
        assert!(!joined.contains("--recode-video"), "must never force a re-encode");
        assert!(!joined.contains("--cookies") && !joined.contains("--proxy"));
        assert!(a.iter().any(|x| x.ends_with("media.%(ext)s")));
        assert!(a.iter().any(|x| x.starts_with("thumbnail:") && x.ends_with("cover.%(ext)s")));

        let audio = Plan { selector: "ba/b".into(), sort: None, audio: true, height: None, est: None };
        let net = NetOpts {
            referer: Some("https://e.com/".into()),
            headers: vec![("Origin".into(), "https://e.com".into())],
            file_urls: true,
            ..NetOpts::default()
        };
        let joined = download_args(&cfg, &net, &audio, dir).join(" ");
        assert!(joined.contains("-x --audio-format m4a>m4a/mp3>mp3/mp4>m4a/mov>m4a/aac>m4a/mp3"));
        assert!(joined.contains("--referer https://e.com/ --add-header Origin:https://e.com --enable-file-urls"));
        assert!(!joined.contains("--merge-output-format"));

        let ipv4_net = NetOpts { force_ipv4: true, ..NetOpts::default() };
        let joined_ipv4 = download_args(&cfg, &ipv4_net, &video, dir).join(" ");
        assert!(joined_ipv4.contains("--force-ipv4"));

        assert_eq!(template_path(Path::new("/a%b"), "media.%(ext)s").replace('\\', "/"), "/a%%b/media.%(ext)s");
    }
}
