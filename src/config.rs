//! Settings from the environment (and an optional `.env` in the working dir).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::i18n::Lang;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transcode {
    /// Re-encode to H.264 only when the result would not play inline and the
    /// clip is short enough (see `transcode_max_secs`).
    Auto,
    /// Never re-encode; unplayable files are sent as documents.
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioFormat {
    /// Keep AAC/MP3 sources as-is (stream copy), encode everything else to MP3.
    Auto,
    /// Always MP3.
    Mp3,
}

#[derive(Debug)]
pub struct Config {
    pub bot_token: String,
    pub api_url: String,
    /// True when `API_SERVER_URL` points at a local Bot API server (2 GB uploads).
    pub local_api: bool,
    /// Hand the local Bot API server a `file://` path instead of streaming the
    /// file over HTTP. Needs the temp dir shared with the server at the same path.
    pub local_api_files: bool,
    pub allowed_users: HashSet<i64>,
    pub admin_users: HashSet<i64>,
    pub default_lang: Lang,

    pub data_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub log_level: log::LevelFilter,
    pub log_file: Option<PathBuf>,

    pub max_parallel: usize,
    /// Downloads one user may run at the same time; the rest wait their turn.
    pub per_user: usize,
    /// Links one user may have queued (waiting + running).
    pub user_queue: usize,
    /// Links queued in total, all users together.
    pub max_queue: usize,
    pub download_timeout: u64,
    pub max_file_bytes: u64,
    /// 0 = unlimited
    pub max_duration: u64,
    pub max_height: u32,
    pub min_free_mb: u64,
    pub block_private: bool,
    pub progress_interval: u64,

    pub concurrent_fragments: u32,
    pub rate_limit: Option<String>,
    pub child_nice: i32,
    pub transcode: Transcode,
    pub transcode_max_secs: u64,
    pub transcode_height: u32,
    pub ffmpeg_threads: u32,
    pub audio_format: AudioFormat,
    pub audio_quality: String,

    pub cookies_file: Option<PathBuf>,
    pub browser_cookies: Option<String>,
    pub proxy_url: Option<String>,
    pub plugin_dir: Option<PathBuf>,
    pub ytdlp_bin: String,
    pub ffmpeg_bin: String,
    pub ffprobe_bin: String,
    pub ytdlp_extra_args: Vec<String>,
    pub ytdlp_update_cmd: Option<String>,
    pub ytdlp_update_hours: u64,
}

/// Process environment layered over a `.env` file (real env wins).
pub struct Env {
    file: HashMap<String, String>,
}

impl Env {
    pub fn load() -> Env {
        let mut file = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(".env") {
            parse_dotenv(&text, &mut file);
        }
        Env { file }
    }

    #[cfg(test)]
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Env {
        Env { file: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let v = match std::env::var(key) {
            Ok(v) => Some(v),
            Err(_) => self.file.get(key).cloned(),
        }?;
        let v = v.trim();
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    }

    fn num<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, String> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("{key}: invalid number '{v}'")),
        }
    }

    fn flag(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            None => default,
            Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        }
    }
}

fn parse_dotenv(text: &str, out: &mut HashMap<String, String>) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        let value = match value.as_bytes().first() {
            Some(b'"') | Some(b'\'') if value.len() >= 2 && value.ends_with(&value[..1]) => &value[1..value.len() - 1],
            // Unquoted: an inline " #comment" is not part of the value.
            _ => value.split(" #").next().unwrap_or(value).trim(),
        };
        out.insert(key.trim().to_string(), value.to_string());
    }
}

impl Config {
    pub fn from_env(env: &Env) -> Result<Config, String> {
        let bot_token = env.get("BOT_TOKEN").ok_or("BOT_TOKEN is required (get one from @BotFather)")?;

        let api_server = env.get("API_SERVER_URL");
        let local_api = api_server.is_some();
        let api_url =
            api_server.unwrap_or_else(|| "https://api.telegram.org".to_string()).trim_end_matches('/').to_string();

        let mut allowed_users = HashSet::new();
        for part in env.get("ALLOWED_USERS").unwrap_or_default().split(',') {
            let part = part.trim();
            if !part.is_empty() {
                allowed_users.insert(part.parse::<i64>().map_err(|_| format!("ALLOWED_USERS: bad id '{part}'"))?);
            }
        }

        let mut admin_users = HashSet::new();
        for part in env.get("ADMIN_USERS").unwrap_or_default().split(',') {
            let part = part.trim();
            if !part.is_empty() {
                let id = part.parse::<i64>().map_err(|_| format!("ADMIN_USERS: bad id '{part}'"))?;
                admin_users.insert(id);
                if !allowed_users.is_empty() {
                    allowed_users.insert(id);
                }
            }
        }

        let default_lang = match env.get("DEFAULT_LANG") {
            None => Lang::En,
            Some(code) => Lang::from_code(&code).ok_or_else(|| format!("DEFAULT_LANG: unknown language '{code}'"))?,
        };

        let data_dir = PathBuf::from(env.get("DATA_DIR").unwrap_or_else(|| "data".to_string()));
        let temp_dir = env.get("TEMP_DIR").map(PathBuf::from).unwrap_or_else(|| data_dir.join("tmp"));

        let log_level = match env.get("LOG_LEVEL").unwrap_or_default().to_ascii_uppercase().as_str() {
            "DEBUG" => log::LevelFilter::Debug,
            "WARNING" | "WARN" => log::LevelFilter::Warn,
            "ERROR" => log::LevelFilter::Error,
            _ => log::LevelFilter::Info,
        };

        let upload_limit_mb: u64 = if local_api { 2000 } else { 50 };
        let max_file_mb = env.num("MAX_FILE_SIZE_MB", upload_limit_mb)?.clamp(1, upload_limit_mb);

        let transcode = match env.get("TRANSCODE").unwrap_or_default().to_ascii_lowercase().as_str() {
            "never" | "off" | "false" => Transcode::Never,
            _ => Transcode::Auto,
        };
        let audio_format = match env.get("AUDIO_FORMAT").unwrap_or_default().to_ascii_lowercase().as_str() {
            "mp3" => AudioFormat::Mp3,
            _ => AudioFormat::Auto,
        };

        let browser_cookies = if env.flag("USE_BROWSER_COOKIES", false) {
            Some(env.get("BROWSER_NAME").unwrap_or_else(|| "firefox".to_string()))
        } else {
            None
        };

        Ok(Config {
            bot_token,
            api_url,
            local_api,
            local_api_files: local_api && env.flag("LOCAL_API_SHARED_FILES", false),
            allowed_users,
            admin_users,
            default_lang,
            data_dir,
            temp_dir,
            log_level,
            log_file: env.get("LOG_FILE").map(PathBuf::from),
            max_parallel: env.num("MAX_PARALLEL_DOWNLOADS", 1usize)?.max(1),
            per_user: env.num("RATE_LIMIT_PER_USER", 1usize)?.max(1),
            user_queue: env.num("USER_QUEUE_SIZE", 100usize)?.clamp(1, 1000),
            max_queue: env.num("MAX_QUEUE_SIZE", 1000usize)?.max(1),
            download_timeout: env.num("DOWNLOAD_TIMEOUT", 1800u64)?.max(30),
            max_file_bytes: max_file_mb * 1_048_576,
            max_duration: env.num("MAX_DURATION_MIN", 180u64)? * 60,
            max_height: env.num("MAX_HEIGHT", 1080u32)?.max(144),
            min_free_mb: env.num("MIN_FREE_DISK_MB", 300u64)?,
            block_private: env.flag("BLOCK_PRIVATE_URLS", true),
            progress_interval: env.num("PROGRESS_INTERVAL", 5u64)?.max(2),
            concurrent_fragments: env.num("CONCURRENT_FRAGMENTS", 4u32)?.clamp(1, 32),
            rate_limit: env.get("DOWNLOAD_RATE_LIMIT"),
            child_nice: env.num("CHILD_NICE", 10i32)?.clamp(0, 19),
            transcode,
            transcode_max_secs: env.num("TRANSCODE_MAX_DURATION", 600u64)?,
            transcode_height: env.num("TRANSCODE_HEIGHT", 720u32)?.max(144),
            ffmpeg_threads: env.num("FFMPEG_THREADS", 1u32)?.max(1),
            audio_format,
            audio_quality: env.get("AUDIO_QUALITY").unwrap_or_else(|| "4".to_string()),
            cookies_file: env.get("COOKIES_FILE").map(PathBuf::from),
            browser_cookies,
            proxy_url: env.get("PROXY_URL"),
            plugin_dir: env.get("PLUGIN_DIR").map(PathBuf::from),
            ytdlp_bin: env.get("YTDLP_PATH").unwrap_or_else(|| "yt-dlp".to_string()),
            ffmpeg_bin: env.get("FFMPEG_PATH").unwrap_or_else(|| "ffmpeg".to_string()),
            ffprobe_bin: env.get("FFPROBE_PATH").unwrap_or_else(|| "ffprobe".to_string()),
            ytdlp_extra_args: env
                .get("YTDLP_EXTRA_ARGS")
                .map(|s| s.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            ytdlp_update_cmd: env.get("YTDLP_UPDATE_CMD"),
            ytdlp_update_hours: env.num("YTDLP_UPDATE_INTERVAL_HOURS", 24u64)?.max(1),
        })
    }

    /// Hard Telegram cap for the configured API endpoint.
    pub fn upload_limit_bytes(&self) -> u64 {
        if self.local_api {
            2000 * 1_048_576
        } else {
            50 * 1_048_576
        }
    }

    /// The cookies.txt to pass to yt-dlp, if it is configured and present.
    pub fn cookies_path(&self) -> Option<&PathBuf> {
        self.cookies_file.as_ref().filter(|p| p.is_file())
    }

    pub fn has_cookies(&self) -> bool {
        self.cookies_path().is_some() || self.browser_cookies.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_parsing() {
        let mut m = HashMap::new();
        parse_dotenv("# comment\nA=1\nexport B = \"two words\" \nC='x#y'\nD=val # trailing\nE=\nbroken line\n", &mut m);
        assert_eq!(m["A"], "1");
        assert_eq!(m["B"], "two words");
        assert_eq!(m["C"], "x#y");
        assert_eq!(m["D"], "val");
        assert_eq!(m["E"], "");
        assert_eq!(m.len(), 5);
    }

    #[test]
    fn defaults_are_vps_friendly() {
        let env = Env::from_pairs(&[("BOT_TOKEN", "1:abc"), ("ALLOWED_USERS", "1, 2,3")]);
        let cfg = Config::from_env(&env).unwrap();
        assert_eq!(cfg.max_parallel, 1);
        assert_eq!((cfg.per_user, cfg.user_queue), (1, 100));
        assert_eq!(cfg.concurrent_fragments, 4);
        assert_eq!(cfg.ffmpeg_threads, 1);
        assert_eq!(cfg.max_file_bytes, 50 * 1_048_576);
        assert_eq!(cfg.allowed_users.len(), 3);
        assert!(!cfg.local_api);
        assert!(cfg.browser_cookies.is_none());
    }

    #[test]
    fn local_api_raises_limits() {
        let env = Env::from_pairs(&[
            ("BOT_TOKEN", "1:abc"),
            ("API_SERVER_URL", "http://telegram-bot-api:8081/"),
            ("MAX_FILE_SIZE_MB", "99999"),
        ]);
        let cfg = Config::from_env(&env).unwrap();
        assert!(cfg.local_api);
        assert_eq!(cfg.api_url, "http://telegram-bot-api:8081");
        assert_eq!(cfg.max_file_bytes, 2000 * 1_048_576);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Config::from_env(&Env::from_pairs(&[])).is_err());
        let env = Env::from_pairs(&[("BOT_TOKEN", "1:abc"), ("ALLOWED_USERS", "1,x")]);
        assert!(Config::from_env(&env).is_err());
    }

    #[test]
    fn admin_users_parsing() {
        let env = Env::from_pairs(&[("BOT_TOKEN", "1:abc"), ("ALLOWED_USERS", "10,20"), ("ADMIN_USERS", "20, 30")]);
        let cfg = Config::from_env(&env).unwrap();
        assert_eq!(cfg.admin_users.len(), 2);
        assert!(cfg.admin_users.contains(&20) && cfg.admin_users.contains(&30));
        assert!(cfg.allowed_users.contains(&30));
    }
}
