//! Post-download handling with ffprobe/ffmpeg.
//!
//! The rule that keeps a small server alive: never re-encode video unless the
//! file would otherwise be unplayable *and* it is short. Everything else is
//! either sent as-is or remuxed (a stream copy that costs almost no CPU).

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::watch;

use crate::config::{Config, Transcode};
use crate::proc;
use crate::telegram::MediaKind;
use crate::types::{Kind, Stage};
use crate::util;
use crate::ytdlp::{Fetched, Probed};

/// Telegram thumbnails: JPEG, at most 320 px on a side and 200 KB.
const THUMB_MAX_BYTES: u64 = 200 * 1024;

#[derive(Deserialize, Default)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: ProbeFormat,
}

#[derive(Deserialize, Default)]
struct ProbeStream {
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    disposition: Disposition,
    #[serde(default)]
    side_data_list: Vec<SideData>,
}

#[derive(Deserialize, Default)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize, Default)]
struct SideData {
    rotation: Option<f64>,
}

#[derive(Deserialize, Default)]
struct ProbeFormat {
    #[serde(default)]
    format_name: String,
    duration: Option<String>,
}

/// What ffprobe says about a file.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MediaInfo {
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    /// Display dimensions (rotation applied).
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: f64,
    pub is_mp4: bool,
}

fn parse_probe(json: &str) -> Option<MediaInfo> {
    let out: ProbeOut = serde_json::from_str(json).ok()?;
    let mut info = MediaInfo {
        duration: out.format.duration.as_deref().and_then(|d| d.parse().ok()).unwrap_or(0.0),
        // ffprobe reports the mp4 family as "mov,mp4,m4a,3gp,3g2,mj2".
        is_mp4: out.format.format_name.split(',').any(|f| f == "mp4"),
        ..MediaInfo::default()
    };
    for s in &out.streams {
        match s.codec_type.as_str() {
            // Embedded cover art shows up as a video stream; skip it.
            "video" if s.disposition.attached_pic == 0 && info.vcodec.is_none() => {
                info.vcodec = Some(s.codec_name.clone());
                let rotated = s.side_data_list.iter().filter_map(|d| d.rotation).any(|r| (r.abs() as i64) % 180 == 90);
                (info.width, info.height) = if rotated { (s.height, s.width) } else { (s.width, s.height) };
            }
            "audio" if info.acodec.is_none() => info.acodec = Some(s.codec_name.clone()),
            _ => {}
        }
    }
    Some(info)
}

pub async fn ffprobe(cfg: &Config, path: &Path) -> Option<MediaInfo> {
    let mut cmd = proc::command(&cfg.ffprobe_bin, cfg.child_nice);
    cmd.args([
        "-v",
        "error",
        "-show_entries",
        "stream=codec_type,codec_name,width,height:stream_disposition=attached_pic:stream_side_data=rotation:format=format_name,duration",
        "-of",
        "json",
    ])
    .arg(path);
    let (out, json) = proc::capture(cmd, Duration::from_secs(60)).await.ok()?;
    if !out.ok {
        log::debug!("ffprobe failed: {}", out.stderr.trim());
        return None;
    }
    parse_probe(&json)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Send as a video without touching it.
    Keep,
    /// Stream-copy into MP4; re-encode only the audio track if `aac` is set.
    Remux { aac: bool },
    /// Full H.264/AAC re-encode (the expensive path).
    Transcode,
    /// Not playable inline and not worth converting: send as a file.
    Document,
}

fn decide(info: &MediaInfo, cfg: &Config) -> Action {
    let Some(vcodec) = info.vcodec.as_deref() else {
        return Action::Document;
    };
    // Plays inline in every Telegram client.
    let v_ok = matches!(vcodec, "h264" | "hevc");
    // Fits in an MP4 container and plays in most (not all) clients.
    let v_mp4 = v_ok || matches!(vcodec, "vp9" | "av1" | "mpeg4");
    let a_ok = info.acodec.as_deref().is_none_or(|a| matches!(a, "aac" | "mp3"));

    if v_ok {
        return if info.is_mp4 && a_ok { Action::Keep } else { Action::Remux { aac: !a_ok } };
    }
    let short = info.duration > 0.0 && info.duration <= cfg.transcode_max_secs as f64;
    if cfg.transcode == Transcode::Auto && short {
        return Action::Transcode;
    }
    if !v_mp4 {
        return Action::Document;
    }
    if info.is_mp4 {
        Action::Keep
    } else {
        // Opus in MP4 is fine wherever VP9/AV1 plays; other codecs become AAC.
        let aac = !a_ok && info.acodec.as_deref() != Some("opus");
        Action::Remux { aac }
    }
}

fn ffmpeg_args(action: Action, info: &MediaInfo, cfg: &Config, input: &Path, output: &Path) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let threads = cfg.ffmpeg_threads.to_string();
    let mut a = vec![s("-hide_banner"), s("-loglevel"), s("error"), s("-nostdin"), s("-y")];
    if action == Action::Transcode {
        // Also caps decoder threads.
        a.extend([s("-threads"), threads.clone()]);
    }
    a.extend([s("-i"), input.display().to_string(), s("-map"), s("0:v:0"), s("-map"), s("0:a:0?")]);
    match action {
        Action::Transcode => {
            let scale = format!("scale=-2:'trunc(min({},ih)/2)*2'", cfg.transcode_height);
            a.extend([
                s("-c:v"),
                s("libx264"),
                s("-preset"),
                s("veryfast"),
                s("-crf"),
                s("26"),
                s("-vf"),
                scale,
                s("-pix_fmt"),
                s("yuv420p"),
                s("-threads"),
                threads,
                s("-filter_threads"),
                s("1"),
                s("-c:a"),
                s("aac"),
                s("-b:a"),
                s("128k"),
                s("-max_muxing_queue_size"),
                s("1024"),
                s("-progress"),
                s("pipe:1"),
                s("-nostats"),
            ]);
        }
        Action::Remux { aac } => {
            a.extend([s("-c:v"), s("copy")]);
            if info.vcodec.as_deref() == Some("hevc") {
                // Apple players only accept HEVC tagged as hvc1.
                a.extend([s("-tag:v"), s("hvc1")]);
            }
            if aac {
                a.extend([s("-c:a"), s("aac"), s("-b:a"), s("128k")]);
            } else {
                a.extend([s("-c:a"), s("copy")]);
            }
        }
        Action::Keep | Action::Document => {}
    }
    a.extend([s("-movflags"), s("+faststart"), output.display().to_string()]);
    a
}

/// Run ffmpeg for `action`, replacing `input` with the new file on success.
async fn convert(
    cfg: &Config,
    action: Action,
    info: &MediaInfo,
    input: &Path,
    stage: &watch::Sender<Stage>,
) -> Option<PathBuf> {
    let output = input.with_file_name("converted.mp4");
    let mut cmd = proc::command(&cfg.ffmpeg_bin, cfg.child_nice);
    cmd.args(ffmpeg_args(action, info, cfg, input, &output));
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let transcoding = action == Action::Transcode;
    stage.send_replace(if transcoding { Stage::Transcoding { pct: None } } else { Stage::Processing });
    let duration_us = info.duration * 1_000_000.0;
    let result = proc::run(cmd, Duration::from_secs(cfg.download_timeout), |line| {
        // `-progress` output: "out_time_us=1234567"
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
            if transcoding && duration_us > 0.0 {
                let pct = (us / duration_us * 100.0).clamp(0.0, 100.0) as f32;
                stage.send_replace(Stage::Transcoding { pct: Some(pct) });
            }
        }
        ControlFlow::Continue(())
    })
    .await;

    let produced = output.metadata().map(|m| m.len() > 0).unwrap_or(false);
    match result {
        Ok(out) if out.ok && produced => {
            let _ = std::fs::remove_file(input);
            Some(output)
        }
        Ok(out) => {
            log::warn!("ffmpeg {action:?} failed: {}", out.stderr.trim());
            let _ = std::fs::remove_file(&output);
            None
        }
        Err(e) => {
            log::warn!("ffmpeg {action:?} failed: {e:?}");
            let _ = std::fs::remove_file(&output);
            None
        }
    }
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 1 < data.len() {
        if data[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xff || marker == 0x00 {
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if i + 2 > data.len() {
            break;
        }
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        if len < 2 || i + len > data.len() {
            break;
        }
        let is_sof = matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf);
        if is_sof && len >= 7 {
            let height = u16::from_be_bytes([data[i + 3], data[i + 4]]) as u32;
            let width = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            return Some((width, height));
        }
        i += len;
    }
    None
}

fn already_valid_thumb(src: &Path) -> bool {
    let ext = src.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase());
    if !matches!(ext.as_deref(), Some("jpg" | "jpeg")) {
        return false;
    }
    let Ok(meta) = src.metadata() else { return false };
    let size = meta.len();
    if size == 0 || size > THUMB_MAX_BYTES {
        return false;
    }
    let Ok(data) = std::fs::read(src) else { return false };
    if let Some((w, h)) = jpeg_dimensions(&data) {
        if w <= 320 && h <= 320 {
            return true;
        }
    }
    false
}

/// Shrink cover art to Telegram's thumbnail limits. Failure is non-fatal.
async fn make_thumb(cfg: &Config, src: &Path) -> Option<PathBuf> {
    if already_valid_thumb(src) {
        log::debug!("cover art already fits thumbnail limits: {}", src.display());
        return Some(src.to_path_buf());
    }
    let dst = src.with_file_name("thumb.jpg");
    let mut cmd = proc::command(&cfg.ffmpeg_bin, cfg.child_nice);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-threads", "1", "-i"])
        .arg(src)
        .args(["-vf", "scale=320:320:force_original_aspect_ratio=decrease", "-frames:v", "1", "-q:v", "5"])
        .arg(&dst);
    cmd.stderr(Stdio::piped());
    let out = proc::run(cmd, Duration::from_secs(60), |_| ControlFlow::Continue(())).await.ok()?;
    let size = dst.metadata().map(|m| m.len()).unwrap_or(0);
    if out.ok && size > 0 && size <= THUMB_MAX_BYTES {
        Some(dst)
    } else {
        log::debug!("thumbnail not usable ({size} bytes): {}", out.stderr.trim());
        None
    }
}

/// A file ready to be sent.
pub struct Ready {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: MediaKind,
    pub title: String,
    pub performer: String,
    pub duration: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumb: Option<PathBuf>,
    pub size: u64,
    pub probed: Probed,
}

/// Inspect the download, convert it only if needed, and gather upload metadata.
pub async fn prepare(cfg: &Config, fetched: Fetched, stage: &watch::Sender<Stage>) -> Ready {
    let Fetched { mut path, thumb, title, performer, duration, probed } = fetched;
    let kind = probed.kind;

    // Fast path: for audio downloads where duration is already known, skip ffprobe entirely.
    if kind == Kind::Audio && duration > 0.0 {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if matches!(ext.as_str(), "mp3" | "m4a" | "aac" | "ogg" | "flac") {
            let thumb = match thumb {
                Some(src) => make_thumb(cfg, &src).await,
                _ => None,
            };
            let ext_str = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "bin".into());
            let file_name = format!("{}.{ext_str}", util::sanitize_filename(&title, 120));
            let named = path.with_file_name(&file_name);
            if named != path && std::fs::rename(&path, &named).is_ok() {
                path = named;
            }
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            return Ready {
                path,
                file_name,
                kind: MediaKind::Audio,
                title,
                performer,
                duration: duration.round().max(0.0) as u32,
                width: None,
                height: None,
                thumb,
                size,
                probed,
            };
        }
    }

    let mut info = ffprobe(cfg, &path).await.unwrap_or_default();
    if info.duration <= 0.0 {
        info.duration = duration;
    }

    let media_kind = if kind == Kind::Audio || info.vcodec.is_none() {
        if info.acodec.is_some() || kind == Kind::Audio {
            MediaKind::Audio
        } else {
            MediaKind::Document
        }
    } else {
        match decide(&info, cfg) {
            Action::Keep => MediaKind::Video,
            Action::Document => MediaKind::Document,
            action => match convert(cfg, action, &info, &path, stage).await {
                Some(converted) => {
                    path = converted;
                    if action == Action::Transcode {
                        // Dimensions changed: calculate scaled dimensions directly instead of running ffprobe again.
                        let target_h = cfg.transcode_height;
                        if let Some(h) = info.height {
                            if h > target_h {
                                let new_h = target_h;
                                let new_w =
                                    info.width.map(|w| (w as f64 * new_h as f64 / h as f64).round() as u32 / 2 * 2);
                                info.height = Some(new_h);
                                info.width = new_w;
                            }
                        } else {
                            info.height = Some(target_h);
                        }
                        info.vcodec = Some("h264".into());
                        info.acodec = Some("aac".into());
                        info.is_mp4 = true;
                    }
                    MediaKind::Video
                }
                None => MediaKind::Document,
            },
        }
    };

    let thumb = match thumb {
        Some(src) if media_kind != MediaKind::Document => make_thumb(cfg, &src).await,
        _ => None,
    };

    // Give the file a human name: it is what Telegram shows for documents.
    let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "bin".into());
    let file_name = format!("{}.{ext}", util::sanitize_filename(&title, 120));
    let named = path.with_file_name(&file_name);
    if named != path && std::fs::rename(&path, &named).is_ok() {
        path = named;
    }
    let size = path.metadata().map(|m| m.len()).unwrap_or(0);

    Ready {
        path,
        file_name,
        kind: media_kind,
        title,
        performer,
        duration: info.duration.round().max(0.0) as u32,
        width: info.width,
        height: info.height,
        thumb,
        size,
        probed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Env;

    fn cfg(pairs: &[(&str, &str)]) -> Config {
        let mut all = vec![("BOT_TOKEN", "1:x")];
        all.extend_from_slice(pairs);
        Config::from_env(&Env::from_pairs(&all)).unwrap()
    }

    fn mi(v: Option<&str>, a: Option<&str>, mp4: bool, duration: f64) -> MediaInfo {
        MediaInfo {
            vcodec: v.map(String::from),
            acodec: a.map(String::from),
            is_mp4: mp4,
            duration,
            ..MediaInfo::default()
        }
    }

    #[test]
    fn parses_ffprobe_output() {
        let json = r#"{
          "streams": [
            {"codec_name": "mjpeg", "codec_type": "video", "width": 500, "height": 500, "disposition": {"attached_pic": 1}},
            {"codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080, "disposition": {"attached_pic": 0},
             "side_data_list": [{"rotation": -90}]},
            {"codec_name": "aac", "codec_type": "audio"}
          ],
          "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "12.500000"}
        }"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(info.vcodec.as_deref(), Some("h264"));
        assert_eq!(info.acodec.as_deref(), Some("aac"));
        assert_eq!((info.width, info.height), (Some(1080), Some(1920)));
        assert!(info.is_mp4);
        assert_eq!(info.duration, 12.5);

        let audio = parse_probe(
            r#"{"streams": [{"codec_name": "mp3", "codec_type": "audio"}], "format": {"format_name": "mp3"}}"#,
        )
        .unwrap();
        assert_eq!((audio.vcodec, audio.is_mp4), (None, false));
        assert!(parse_probe("not json").is_none());
    }

    #[test]
    fn avoids_reencoding_whenever_possible() {
        let auto = cfg(&[]);
        // The common case: H.264/AAC MP4 is never touched.
        assert_eq!(decide(&mi(Some("h264"), Some("aac"), true, 9999.0), &auto), Action::Keep);
        // Wrong container or audio codec: cheap remux, never a video re-encode.
        assert_eq!(decide(&mi(Some("h264"), Some("aac"), false, 9999.0), &auto), Action::Remux { aac: false });
        assert_eq!(decide(&mi(Some("h264"), Some("opus"), true, 60.0), &auto), Action::Remux { aac: true });
        // Short WebM clip: worth converting so it plays inline.
        assert_eq!(decide(&mi(Some("vp8"), Some("vorbis"), false, 30.0), &auto), Action::Transcode);
        // Long non-H.264: no re-encode, whatever the codec.
        assert_eq!(decide(&mi(Some("vp9"), Some("opus"), true, 3600.0), &auto), Action::Keep);
        assert_eq!(decide(&mi(Some("vp9"), Some("opus"), false, 3600.0), &auto), Action::Remux { aac: false });
        assert_eq!(decide(&mi(Some("vp8"), Some("vorbis"), false, 3600.0), &auto), Action::Document);
        // Unknown duration is treated as "long".
        assert_eq!(decide(&mi(Some("vp8"), None, false, 0.0), &auto), Action::Document);

        let never = cfg(&[("TRANSCODE", "never")]);
        assert_eq!(decide(&mi(Some("vp8"), Some("vorbis"), false, 30.0), &never), Action::Document);
        assert_eq!(decide(&mi(Some("av1"), Some("aac"), false, 30.0), &never), Action::Remux { aac: false });
    }

    #[test]
    fn ffmpeg_command_lines() {
        let c = cfg(&[("FFMPEG_THREADS", "2"), ("TRANSCODE_HEIGHT", "480")]);
        let info = mi(Some("vp9"), Some("opus"), false, 10.0);
        let t = ffmpeg_args(Action::Transcode, &info, &c, Path::new("in.webm"), Path::new("out.mp4")).join(" ");
        assert!(t.contains("-threads 2 -i in.webm"));
        assert!(t.contains("-c:v libx264 -preset veryfast"));
        assert!(t.contains("scale=-2:'trunc(min(480,ih)/2)*2'"));
        assert!(t.ends_with("-movflags +faststart out.mp4"));

        let hevc = mi(Some("hevc"), Some("aac"), false, 10.0);
        let r =
            ffmpeg_args(Action::Remux { aac: false }, &hevc, &c, Path::new("in.mkv"), Path::new("out.mp4")).join(" ");
        assert!(r.contains("-c:v copy -tag:v hvc1 -c:a copy"));
        assert!(!r.contains("libx264"));
    }

    #[test]
    fn parses_jpeg_dimensions() {
        assert_eq!(jpeg_dimensions(&[]), None);
        assert_eq!(jpeg_dimensions(&[0xff, 0xd8, 0xff, 0xd9]), None);
        let mut jpeg = vec![0xff, 0xd8];
        // SOF0: len=11, precision=8, height=240, width=320, 1 component (grayscale)
        let sof = [0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0xf0, 0x01, 0x40, 0x01, 0x01, 0x11, 0x00];
        jpeg.extend_from_slice(&sof);
        assert_eq!(jpeg_dimensions(&jpeg), Some((320, 240)));
    }
}
