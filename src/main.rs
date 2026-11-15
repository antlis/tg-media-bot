//! tg-media-bot — lightweight self-hosted Telegram media downloader.
//!
//! The bot process itself is a single-threaded async loop that idles at a few
//! MB of RAM; the heavy work happens in yt-dlp/ffmpeg children that are
//! bounded, niced and killed reliably.

mod app;
mod bot;
mod cachekey;
mod config;
mod i18n;
mod logger;
mod media;
mod meta;
mod plugins;
mod proc;
mod state;
mod sys;
mod telegram;
mod types;
mod util;
mod ytdlp;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::app::App;
use crate::config::{Config, Env};
use crate::i18n::Lang;
use crate::plugins::Plugins;
use crate::state::Store;
use crate::telegram::{Bot, TgError};
use crate::types::{Kind, Stage};

const POLL_TIMEOUT: u64 = 50;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("tg-media-bot {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let health_mode = args.get(1).is_some_and(|a| a == "health" || a == "healthcheck");
    if health_mode {
        let env = Env::load();
        let data_dir = std::path::PathBuf::from(env.get("DATA_DIR").unwrap_or_else(|| "data".to_string()));
        let temp_dir = env.get("TEMP_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| data_dir.join("tmp"));
        let health_file = temp_dir.join(".health");
        let ok = std::fs::metadata(&health_file)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default().as_secs() < 60)
            .unwrap_or(false);
        std::process::exit(if ok { 0 } else { 1 });
    }
    let fetch_mode = args.get(1).is_some_and(|a| a == "fetch");
    if fetch_mode && std::env::var_os("BOT_TOKEN").is_none() {
        std::env::set_var("BOT_TOKEN", "0:fetch-mode");
    }

    let cfg = match Config::from_env(&Env::load()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };
    logger::init(cfg.log_level, if fetch_mode { None } else { cfg.log_file.clone() });

    let workers = std::thread::available_parallelism().map(|n| n.get().min(4)).unwrap_or(2);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = if fetch_mode { rt.block_on(fetch_cli(cfg, &args[2..])) } else { rt.block_on(run(cfg)) };
    std::process::exit(code);
}

pub struct StartupStats {
    pub cleaned_dirs: usize,
    pub corrupt_state: bool,
}

fn prepare_dirs(cfg: &Config) -> Result<StartupStats, String> {
    std::fs::create_dir_all(&cfg.data_dir).map_err(|e| format!("{}: {e}", cfg.data_dir.display()))?;
    std::fs::create_dir_all(&cfg.temp_dir).map_err(|e| format!("{}: {e}", cfg.temp_dir.display()))?;
    let mut cleaned_dirs = 0;
    // Leftovers of a crash or a hard kill.
    if let Ok(entries) = std::fs::read_dir(&cfg.temp_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("job-") && std::fs::remove_dir_all(entry.path()).is_ok() {
                cleaned_dirs += 1;
            }
        }
    }
    let corrupt_state = cfg.data_dir.join("state.json.corrupt").exists();
    if sys::is_tmpfs(&cfg.temp_dir) {
        log::warn!(
            "TEMP_DIR {} is on tmpfs: downloads will consume RAM, not disk. Point TEMP_DIR at a disk-backed directory.",
            cfg.temp_dir.display()
        );
    }
    Ok(StartupStats { cleaned_dirs, corrupt_state })
}

async fn check_tools(cfg: &Config) -> bool {
    match ytdlp::version(cfg).await {
        Some(v) => log::info!("yt-dlp {v}"),
        None => {
            log::error!("yt-dlp not found or not working ({}); install it or set YTDLP_PATH", cfg.ytdlp_bin);
            return false;
        }
    }
    for bin in [&cfg.ffmpeg_bin, &cfg.ffprobe_bin] {
        if sys::which(bin).is_none() {
            log::warn!("{bin} not found: merging, audio extraction and thumbnails will fail");
        }
    }
    true
}

async fn shutdown_signal(tx: watch::Sender<bool>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
    log::info!("shutting down…");
    let _ = tx.send(true);
}

async fn register_commands(app: Arc<App>) {
    let default = bot::command_list(app.cfg.default_lang.t());
    if let Err(e) = app.bot.set_commands(&default, None, None).await {
        log::warn!("setMyCommands failed: {e}");
    }
    for lang in Lang::ALL {
        if let Err(e) = app.bot.set_commands(&bot::command_list(lang.t()), Some(lang.code()), None).await {
            log::warn!("setMyCommands({}) failed: {e}", lang.code());
        }
    }
}

/// Periodically refresh yt-dlp (sites change constantly), but only while no
/// download is running: all slots are held during the update.
async fn updater(app: Arc<App>) {
    let Some(cmdline) = app.cfg.ytdlp_update_cmd.clone() else { return };
    let mut delay = Duration::from_secs(120);
    loop {
        tokio::time::sleep(delay).await;
        let Ok(_all) = app.slots.acquire_many(app.cfg.max_parallel as u32).await else { return };
        #[cfg(unix)]
        let mut cmd = {
            let mut c = proc::command("sh", app.cfg.child_nice);
            c.arg("-c").arg(&cmdline);
            c
        };
        #[cfg(not(unix))]
        let mut cmd = {
            let mut c = proc::command("cmd", app.cfg.child_nice);
            c.arg("/C").arg(&cmdline);
            c
        };
        cmd.stderr(std::process::Stdio::piped());
        match proc::capture(cmd, Duration::from_secs(600)).await {
            Ok((out, _)) if out.ok => {
                log::info!("yt-dlp update ok: {}", ytdlp::version(&app.cfg).await.unwrap_or_default())
            }
            Ok((out, _)) => log::warn!("yt-dlp update failed: {}", out.stderr.trim()),
            Err(e) => log::warn!("yt-dlp update failed: {e:?}"),
        }
        delay = Duration::from_secs(app.cfg.ytdlp_update_hours * 3600);
    }
}

async fn housekeeping(app: Arc<App>) {
    let health_file = app.cfg.temp_dir.join(".health");
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tick.tick().await;
        let _ = tokio::fs::write(&health_file, state::now_secs().to_string()).await;
        app.store.flush_async().await;
    }
}

async fn run(cfg: Config) -> i32 {
    let stats = match prepare_dirs(&cfg) {
        Ok(s) => s,
        Err(e) => {
            log::error!("{e}");
            return 2;
        }
    };
    if !check_tools(&cfg).await {
        return 2;
    }
    if cfg.allowed_users.is_empty() {
        log::warn!("ALLOWED_USERS is empty: ANYONE can use this bot and your server's bandwidth");
    }

    let (stop_tx, mut stop) = watch::channel(false);
    tokio::spawn(shutdown_signal(stop_tx));

    let mut bot = match Bot::new(&cfg) {
        Ok(b) => b,
        Err(e) => {
            log::error!("{e}");
            return 2;
        }
    };
    let mut backoff = 2;
    loop {
        match bot.init().await {
            Ok(()) => break,
            Err(TgError::Api { code: 401 | 404, desc, .. }) => {
                log::error!("Telegram rejected BOT_TOKEN: {desc}");
                return 2;
            }
            Err(e) => {
                log::warn!("getMe failed ({e}); retrying in {backoff}s");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                    _ = stop.changed() => return 0,
                }
                backoff = (backoff * 2).min(60);
            }
        }
    }
    log::info!(
        "@{} online · {} · {} parallel · up to {} per file",
        bot.username,
        if cfg.local_api { "local Bot API" } else { "api.telegram.org" },
        cfg.max_parallel,
        util::fmt_size(cfg.max_file_bytes)
    );

    let store = Store::load(&cfg.data_dir);
    let plugins = Plugins::load(cfg.plugin_dir.as_deref());
    let app = Arc::new(App::new(cfg, bot, store, plugins));
    tokio::spawn(register_commands(app.clone()));
    tokio::spawn(housekeeping(app.clone()));
    tokio::spawn(updater(app.clone()));

    // Recover interrupted jobs from previous run / crash
    let saved_jobs = app.store.take_active_jobs();
    let (resumed, dropped) = app.recover_jobs(saved_jobs).await;

    if stats.cleaned_dirs > 0 || dropped > 0 || resumed > 0 || stats.corrupt_state {
        let admin_ids: Vec<i64> = if !app.cfg.admin_users.is_empty() {
            app.cfg.admin_users.iter().copied().collect()
        } else {
            app.cfg.allowed_users.iter().copied().collect()
        };
        if !admin_ids.is_empty() {
            let free_mb =
                sys::free_space_mb(&app.cfg.temp_dir).map(|d| format!("{d} MB")).unwrap_or_else(|| "n/a".into());
            let alert = format!(
                "⚠️ <b>tg-media-bot: Restart & Recovery Report</b>\n\n\
                 • Resumed jobs: <b>{resumed}</b>\n\
                 • Dropped interrupted jobs: <b>{dropped}</b>\n\
                 • Cleaned temporary directories: <b>{}</b>\n\
                 • Corrupt state backup detected: <b>{}</b>\n\
                 • Disk free: <b>{free_mb}</b>",
                stats.cleaned_dirs,
                if stats.corrupt_state { "yes" } else { "no" }
            );
            for admin in admin_ids {
                app.say(admin, None, &alert, None).await;
            }
        }
    }

    let mut offset = 0i64;
    let mut backoff = 1u64;
    loop {
        let polled = tokio::select! {
            r = app.bot.get_updates(offset, POLL_TIMEOUT) => r,
            _ = stop.changed() => break,
        };
        match polled {
            Ok((next, updates)) => {
                backoff = 1;
                offset = next;
                for update in updates {
                    tokio::spawn(bot::handle(app.clone(), update));
                }
            }
            Err(e) => {
                let wait = match &e {
                    TgError::Api { code: 409, .. } => {
                        log::error!("another instance is polling with this token (or a webhook is set): {e}");
                        30
                    }
                    _ => e.retry_after().unwrap_or(backoff),
                };
                log::warn!("getUpdates failed ({e}); retrying in {wait}s");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
                    _ = stop.changed() => break,
                }
                backoff = (backoff * 2).min(30);
            }
        }
    }

    // Acknowledge the updates we already handled so they are not replayed.
    if offset != 0 {
        let _ = tokio::time::timeout(Duration::from_secs(5), app.bot.get_updates(offset, 0)).await;
    }
    let running = app.active_jobs();
    if running > 0 {
        log::info!("aborting {running} active job(s)");
    }
    app.abort_all();
    // Let aborted tasks drop (killing their children and removing temp dirs).
    tokio::time::sleep(Duration::from_millis(300)).await;
    let _ = std::fs::remove_file(app.cfg.temp_dir.join(".health"));
    app.store.flush();
    log::info!("bye");
    0
}

/// `tg-media-bot fetch <url> [audio|<height>]` — run the download pipeline
/// locally without Telegram, to check yt-dlp/ffmpeg and the limits.
async fn fetch_cli(cfg: Config, args: &[String]) -> i32 {
    let Some(url) = args.first() else {
        eprintln!("usage: tg-media-bot fetch <url> [audio|<height>]");
        return 2;
    };
    let (kind, cap) = match args.get(1).map(String::as_str) {
        Some("audio") => (Kind::Audio, None),
        Some(h) => (Kind::Video, h.parse().ok()),
        None => (Kind::Video, None),
    };
    if prepare_dirs(&cfg).is_err() || !check_tools(&cfg).await {
        return 2;
    }
    let dir = cfg.temp_dir.join(format!("fetch-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("{}: {e}", dir.display());
        return 2;
    }
    let (stage, mut rx) = watch::channel(Stage::Queued);
    let printer = tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let s = rx.borrow_and_update().clone();
            eprintln!("  {s:?}");
        }
    });
    let plugins = Plugins::load(cfg.plugin_dir.as_deref());
    let started = std::time::Instant::now();
    let result = ytdlp::fetch(&cfg, &plugins, url, kind, cap, &dir, &stage, &|_| None, &|_| ()).await;
    let code = match result {
        Err(e) => {
            eprintln!("failed: {e:?}");
            if let ytdlp::DlError::Failed(raw) = &e {
                eprintln!("hint: {:?}", ytdlp::classify(raw));
            }
            1
        }
        Ok(ytdlp::Outcome::Cached(..)) => unreachable!("fetch mode has no cache"),
        Ok(ytdlp::Outcome::Downloaded(fetched)) => {
            let ready = media::prepare(&cfg, fetched, &stage).await;
            println!("video id: {:?}", ready.probed.video_key);
            println!("quality:  {:?}", ready.probed.height);
            println!("file:     {}", ready.path.display());
            println!("send as:  {:?}", ready.kind);
            println!("size:     {}", util::fmt_size(ready.size));
            println!("title:    {}", ready.title);
            println!("duration: {}", util::fmt_duration(ready.duration as u64));
            println!("video:    {:?}x{:?}", ready.width, ready.height);
            println!("thumb:    {:?}", ready.thumb);
            println!("took:     {:.1?}", started.elapsed());
            0
        }
    };
    drop(stage);
    let _ = printer.await;
    code
}
