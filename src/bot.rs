//! Update routing: access control, commands, links and button presses.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::app::{cancel_markup, render_status, stage_label, App, Cancel, Refusal, Request};
use crate::i18n::{fill, Lang, Strings, LANG_PROMPT};
use crate::sys;
use crate::telegram::{CallbackQuery, Message, Update, User};
use crate::types::{Quality, Stage};
use crate::util;

/// Links taken from one message.
const MAX_LINKS_PER_MESSAGE: usize = 50;
/// Lines of /status before "…and N more".
const STATUS_LINES: usize = 15;

/// Telegram command menu for one language.
pub fn command_list(t: &Strings) -> Vec<(&'static str, &'static str)> {
    vec![
        ("start", t.c_start),
        ("help", t.c_help),
        ("quality", t.c_quality),
        ("status", t.c_status),
        ("cancel", t.c_cancel),
        ("clear", t.c_clear),
        ("lang", t.c_lang),
        ("minimal", t.c_minimal),
        ("topic", t.c_topic),
    ]
}

fn lang_keyboard() -> Value {
    let buttons: Vec<Value> =
        Lang::ALL.iter().map(|l| json!({ "text": l.button(), "callback_data": format!("l:{}", l.code()) })).collect();
    let rows: Vec<Value> = buttons.chunks(2).map(|c| Value::Array(c.to_vec())).collect();
    json!({ "inline_keyboard": rows })
}

/// Buttons for every quality choice; `data` builds the callback payload.
fn quality_rows(t: &Strings, max_height: u32, data: impl Fn(&str) -> String) -> Vec<Vec<Value>> {
    let button = |q: Quality| json!({ "text": q.label(t), "callback_data": data(&q.code()) });
    let mut rows = vec![vec![button(Quality::Max), button(Quality::Audio)]];
    let heights: Vec<Value> =
        Quality::HEIGHTS.into_iter().filter(|h| *h <= max_height).map(|h| button(Quality::Height(h))).collect();
    for chunk in heights.chunks(2) {
        rows.push(chunk.to_vec());
    }
    rows
}

fn picker_keyboard(t: &Strings, token: &str, max_height: u32) -> Value {
    json!({ "inline_keyboard": quality_rows(t, max_height, |code| format!("q:{token}:{code}")) })
}

fn default_keyboard(t: &Strings, max_height: u32) -> Value {
    let mut rows = quality_rows(t, max_height, |code| format!("d:{code}"));
    rows.push(vec![json!({ "text": t.btn_ask, "callback_data": "d:ask" })]);
    json!({ "inline_keyboard": rows })
}

fn status_menu(app: &App, user_id: i64, lang: Lang) -> (String, Value) {
    let t = lang.t();
    let jobs = app.user_jobs(user_id);
    let mut text = fill(t.status_header, &[("n", &jobs.len().to_string()), ("max", &app.cfg.user_queue.to_string())]);
    let mut ordered: Vec<_> = jobs.iter().collect();
    ordered.sort_by_key(|j| (*j.stage.borrow() == Stage::Queued, j.seq));

    let mut buttons: Vec<Value> = Vec::new();
    for job in ordered.iter().take(STATUS_LINES) {
        let stage = job.stage.borrow().clone();
        let title_line = match job.title() {
            Some(title) if !title.trim().is_empty() => {
                format!("  🎬 <b>{}</b>\n", util::html_escape(util::truncate_chars(&title, 80)))
            }
            _ => format!("  🎬 <i>{}</i>\n", t.st_title_pending),
        };
        text.push_str(&format!(
            "\n\n• <code>{}</code> {} · {}\n{}  🔗 {}",
            job.id,
            stage_label(t, &stage),
            job.quality.label(t),
            title_line,
            util::html_escape(util::truncate_chars(&job.url, 70))
        ));

        let btn_label = match job.title() {
            Some(title) if !title.trim().is_empty() => {
                format!("✖ {} · {}", job.id, util::truncate_chars(&title, 20))
            }
            _ => format!("✖ {}", job.id),
        };
        buttons.push(json!({
            "text": btn_label,
            "callback_data": format!("c:{}", job.id),
        }));
    }

    if jobs.len() > STATUS_LINES {
        text.push('\n');
        text.push_str(&fill(t.status_more, &[("n", &(jobs.len() - STATUS_LINES).to_string())]));
    }

    let mut rows: Vec<Vec<Value>> = buttons.into_iter().map(|b| vec![b]).collect();
    if jobs.len() > 1 {
        rows.push(vec![json!({
            "text": t.btn_cancel_all,
            "callback_data": "c:all",
        })]);
    }

    (text, json!({ "inline_keyboard": rows }))
}

fn quality_name(t: &Strings, q: Option<Quality>) -> String {
    q.map(|q| q.label(t)).unwrap_or_else(|| t.word_ask.to_string())
}

enum Access {
    Granted,
    Denied,
    /// Outside the chat's locked topic: stay silent.
    Ignored,
}

fn check_access(app: &App, msg: &Message, user: Option<&User>, is_topic_cmd: bool) -> Access {
    let chat = &msg.chat;
    if chat.is_group() && !is_topic_cmd {
        if let Some(locked) = app.store.topic(chat.id) {
            if msg.thread() != Some(locked) {
                return Access::Ignored;
            }
        }
    }
    if app.cfg.allowed_users.is_empty() {
        return Access::Granted;
    }
    if user.is_some_and(|u| app.cfg.allowed_users.contains(&u.id)) {
        if chat.is_group() && !app.store.is_active(chat.id) {
            log::info!("group {} activated by an allowed user", chat.id);
            app.store.activate(chat.id);
        }
        return Access::Granted;
    }
    if chat.is_group() && app.store.is_active(chat.id) {
        return Access::Granted;
    }
    Access::Denied
}

pub async fn handle(app: Arc<App>, update: Update) {
    if let Some(msg) = update.message {
        handle_message(&app, msg).await;
    } else if let Some(cb) = update.callback_query {
        handle_callback(&app, cb).await;
    }
}

/// Split "/cmd@bot args" into ("cmd", args). None if addressed to another bot.
fn parse_command<'a>(text: &'a str, bot_username: &str) -> Option<(String, &'a str)> {
    let rest = text.strip_prefix('/')?;
    let (head, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let (name, target) = head.split_once('@').unwrap_or((head, ""));
    if !target.is_empty() && !target.eq_ignore_ascii_case(bot_username) {
        return None;
    }
    Some((name.to_ascii_lowercase(), args.trim()))
}

fn urls_of(msg: &Message, text: &str) -> Vec<String> {
    let mut urls = util::extract_urls(text, MAX_LINKS_PER_MESSAGE);
    for e in msg.entities.iter().chain(&msg.caption_entities) {
        if let Some(u) = e.url.as_deref().filter(|_| e.kind == "text_link") {
            if urls.len() < MAX_LINKS_PER_MESSAGE && util::is_valid_url(u) && !urls.iter().any(|x| x == u) {
                urls.push(u.to_string());
            }
        }
    }
    urls
}

/// Who asked, where, in which language.
struct Ctx {
    chat: i64,
    thread: Option<i64>,
    user: i64,
    lang: Lang,
}

/// Ask which quality to use for `urls` (one question for the whole message).
async fn ask_quality(app: &Arc<App>, ctx: &Ctx, urls: Vec<String>, user_msg_id: Option<i64>) {
    let t = ctx.lang.t();
    let text = if urls.len() == 1 {
        t.ask_quality.to_string()
    } else {
        fill(t.ask_quality_many, &[("n", &urls.len().to_string())])
    };
    let token = app.stash_urls(ctx.user, urls, user_msg_id);
    app.say(ctx.chat, ctx.thread, &text, Some(picker_keyboard(t, &token, app.cfg.max_height))).await;
}

/// Queue `urls` with `quality` and report the result. `picker` is the
/// question message, if there was one: a single link turns it into its status
/// message, several links turn it into a summary.
async fn enqueue_all(
    app: &Arc<App>,
    ctx: &Ctx,
    urls: Vec<String>,
    quality: Quality,
    picker: Option<i64>,
    user_msg_id: Option<i64>,
) {
    let t = ctx.lang.t();
    let single = urls.len() == 1;
    let (mut added, mut skipped) = (0usize, 0usize);
    let mut problems: Vec<String> = Vec::new();
    let mut minimal = app.store.minimal(ctx.chat);

    for url in urls {
        let req = Request {
            chat_id: ctx.chat,
            thread_id: ctx.thread,
            user_id: ctx.user,
            url,
            quality,
            lang: ctx.lang,
            status_msg: if single { picker } else { None },
            user_msg_id,
        };
        match app.enqueue(req) {
            Ok(job) => {
                added += 1;
                minimal = job.minimal;
                if let (true, Some(msg), false) = (single, picker, job.minimal) {
                    // The question becomes this link's status message.
                    let text = render_status(&job);
                    let markup = cancel_markup(t, &job.id);
                    if let Err(e) = app.bot.edit_message(ctx.chat, msg, &text, Some(markup)).await {
                        log::debug!("could not turn the question into a status: {e}");
                    }
                }
            }
            Err(Refusal::UserQueueFull) => skipped += 1,
            Err(r) => {
                let text = r.text(t, 0, 0);
                if !problems.contains(&text) {
                    problems.push(text);
                }
            }
        }
    }

    if single && added == 1 {
        if minimal {
            if let Some(msg) = picker {
                let _ = app.bot.delete_message(ctx.chat, msg).await;
            }
        }
        return;
    }

    let mut lines = Vec::new();
    if added > 0 && !single && skipped == 0 {
        lines.push(fill(t.queued_n, &[("choice", &quality.label(t)), ("n", &added.to_string())]));
    }
    if skipped > 0 {
        let max = app.cfg.user_queue.to_string();
        if added == 0 {
            let queued = app.cfg.user_queue - app.user_room(ctx.user);
            lines.push(fill(t.user_queue_full, &[("n", &queued.to_string()), ("max", &max)]));
        } else {
            lines.push(fill(
                t.queue_partial,
                &[("n", &added.to_string()), ("skipped", &skipped.to_string()), ("max", &max)],
            ));
        }
    }
    lines.extend(problems);
    if lines.is_empty() || (minimal && skipped == 0 && lines.len() == 1 && added > 0) {
        if let Some(msg) = picker {
            let _ = app.bot.delete_message(ctx.chat, msg).await;
        }
        return;
    }
    let text = lines.join("\n");
    let summary_msg: Option<i64> = match picker {
        Some(msg) => {
            if app.bot.edit_message(ctx.chat, msg, &text, None).await.is_err() {
                app.bot.send_message(ctx.chat, ctx.thread, &text, None, user_msg_id).await.ok().map(|m| m.message_id)
            } else {
                Some(msg)
            }
        }
        None => app.bot.send_message(ctx.chat, ctx.thread, &text, None, user_msg_id).await.ok().map(|m| m.message_id),
    };
    if skipped == 0 && added > 0 {
        if let Some(msg_id) = summary_msg {
            let bot = app.bot.clone();
            let chat_id = ctx.chat;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let _ = bot.delete_message(chat_id, msg_id).await;
            });
        }
    }
}

async fn handle_message(app: &Arc<App>, msg: Message) {
    let Some(user) = msg.from.clone() else { return };
    if user.is_bot {
        return;
    }
    let text = msg.text.as_deref().or(msg.caption.as_deref()).unwrap_or("").to_string();
    let command = parse_command(&text, &app.bot.username);
    if text.starts_with('/') && command.is_none() {
        return; // a command for some other bot in the group
    }
    let is_topic_cmd = command.as_ref().is_some_and(|(c, _)| c == "topic");

    let lang = app.lang_for(msg.chat.id, Some(&user));
    let t = lang.t();
    let ctx = Ctx { chat: msg.chat.id, thread: msg.thread(), user: user.id, lang };

    match check_access(app, &msg, Some(&user), is_topic_cmd) {
        Access::Granted => {}
        Access::Ignored => return,
        Access::Denied => {
            log::warn!("denied user {} in chat {}", user.id, ctx.chat);
            // In groups, only answer people who actually talk to the bot.
            if !msg.chat.is_group() || command.is_some() {
                app.say(ctx.chat, ctx.thread, t.not_authorized, None).await;
            }
            return;
        }
    }

    if let Some((cmd, args)) = command {
        run_command(app, &msg, &ctx, &cmd, args).await;
        return;
    }

    let urls = urls_of(&msg, &text);
    if urls.is_empty() {
        let addressed = !msg.chat.is_group()
            || msg.reply_to_message.as_ref().and_then(|r| r.from.as_ref()).is_some_and(|f| f.id == app.bot.id)
            || (!app.bot.username.is_empty()
                && text.to_lowercase().contains(&format!("@{}", app.bot.username.to_lowercase())));
        if addressed && msg.text.is_some() {
            app.say(ctx.chat, ctx.thread, t.nudge, None).await;
        }
        return;
    }

    let room = app.user_room(user.id);
    if room == 0 {
        let text = Refusal::UserQueueFull.text(t, app.cfg.user_queue, app.cfg.user_queue);
        app.say(ctx.chat, ctx.thread, &text, None).await;
        return;
    }
    match app.store.default_quality(user.id) {
        Some(q) => enqueue_all(app, &ctx, urls, q, None, Some(msg.message_id)).await,
        None => ask_quality(app, &ctx, urls, Some(msg.message_id)).await,
    }
}

async fn run_command(app: &Arc<App>, msg: &Message, ctx: &Ctx, cmd: &str, args: &str) {
    let t = ctx.lang.t();
    let (chat, thread) = (ctx.chat, ctx.thread);
    let mut words = args.split_whitespace();
    let first = words.next().map(str::to_ascii_lowercase);

    match cmd {
        "start" => app.say(chat, thread, &format!("{}\n\n{LANG_PROMPT}", t.welcome), Some(lang_keyboard())).await,
        "help" => {
            let mut limits = fill(
                t.help_limits,
                &[("size", &util::fmt_size(app.cfg.max_file_bytes)), ("height", &app.cfg.max_height.to_string())],
            );
            if app.cfg.max_duration > 0 {
                limits.push_str(&fill(t.help_limit_duration, &[("dur", &util::fmt_duration(app.cfg.max_duration))]));
            }
            app.say(chat, thread, &format!("{}\n\n{limits}", t.help), None).await;
        }
        "lang" | "language" => app.say(chat, thread, LANG_PROMPT, Some(lang_keyboard())).await,
        "quality" => {
            let current = quality_name(t, app.store.default_quality(ctx.user));
            let text = fill(t.quality_current, &[("q", &current)]);
            app.say(chat, thread, &text, Some(default_keyboard(t, app.cfg.max_height))).await;
        }
        // Force the question even when a default quality is set.
        "formats" => {
            let urls = urls_of(msg, args);
            if urls.is_empty() {
                app.say(chat, thread, t.formats_usage, None).await;
            } else {
                ask_quality(app, ctx, urls, Some(msg.message_id)).await;
            }
        }
        "status" => {
            let jobs = app.user_jobs(ctx.user);
            if jobs.is_empty() {
                app.say(chat, thread, t.status_empty, None).await;
                return;
            }
            let (text, markup) = status_menu(app, ctx.user, ctx.lang);
            app.say(chat, thread, &text, Some(markup)).await;
        }
        "cancel" => {
            let id = match first.as_deref() {
                Some("all") => {
                    let n = app.cancel_all(ctx.user).await;
                    app.say(chat, thread, &fill(t.cancelled_n, &[("n", &n.to_string())]), None).await;
                    return;
                }
                Some(id) => id.to_string(),
                None => {
                    let jobs = app.user_jobs(ctx.user);
                    if jobs.len() != 1 {
                        app.say(chat, thread, t.cancel_usage, None).await;
                        return;
                    }
                    jobs[0].id.clone()
                }
            };
            match app.cancel(&id, ctx.user).await {
                Cancel::Done => {}
                Cancel::Denied => app.say(chat, thread, t.cancel_denied, None).await,
                Cancel::NotFound => {
                    let text = fill(t.cancel_fail, &[("id", &util::html_escape(&id))]);
                    app.say(chat, thread, &text, None).await;
                }
            }
        }
        "clear" | "clean" => {
            if msg.chat.is_group() && !app.cfg.allowed_users.is_empty() && !app.cfg.allowed_users.contains(&ctx.user) {
                app.say(chat, thread, t.not_authorized, None).await;
                return;
            }

            let count: usize = first.as_deref().and_then(|s| s.parse().ok()).unwrap_or(100).clamp(1, 500);
            let mut remaining = count;
            let mut current_id = msg.message_id;

            while remaining > 0 && current_id > 0 {
                let batch_size = remaining.min(100);
                let start_id = current_id.saturating_sub(batch_size as i64 - 1).max(1);
                let ids: Vec<i64> = (start_id..=current_id).rev().collect();
                if ids.is_empty() {
                    break;
                }
                let _ = app.bot.delete_messages(chat, &ids).await;
                if start_id <= 1 {
                    break;
                }
                current_id = start_id - 1;
                remaining -= ids.len();
            }

            if let Ok(confirm_msg) = app.bot.send_message(chat, thread, t.cleared, None, None).await {
                let bot = app.bot.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let _ = bot.delete_message(chat, confirm_msg.message_id).await;
                });
            }
        }
        "minimal" => match first.as_deref() {
            Some("on") => {
                app.store.set_minimal(chat, true);
                app.say(chat, thread, t.minimal_on, None).await;
            }
            Some("off") => {
                app.store.set_minimal(chat, false);
                app.say(chat, thread, t.minimal_off, None).await;
            }
            _ => {
                let state = if app.store.minimal(chat) { t.word_on } else { t.word_off };
                app.say(chat, thread, &fill(t.minimal_status, &[("state", state)]), None).await;
            }
        },
        "topic" => {
            if !msg.chat.is_group() {
                app.say(chat, thread, t.topic_groups_only, None).await;
                return;
            }
            match first.as_deref().unwrap_or("status") {
                "lock" => match msg.thread() {
                    None => app.say(chat, thread, t.topic_lock_outside, None).await,
                    Some(topic) => {
                        app.store.set_topic(chat, Some(topic));
                        app.say(chat, thread, t.topic_locked, None).await;
                    }
                },
                "unlock" => {
                    app.store.set_topic(chat, None);
                    app.say(chat, thread, t.topic_unlocked, None).await;
                }
                "status" => {
                    let text = match app.store.topic(chat) {
                        None => t.topic_none.to_string(),
                        Some(id) => fill(t.topic_status, &[("id", &id.to_string())]),
                    };
                    app.say(chat, thread, &text, None).await;
                }
                _ => app.say(chat, thread, t.topic_usage, None).await,
            }
        }
        "stats" | "admin" => {
            if !app.is_admin(ctx.user) {
                app.say(chat, thread, t.not_authorized, None).await;
                return;
            }
            let uptime = util::fmt_duration(app.uptime().as_secs());
            let active = app.active_jobs();
            let cache_entries = app.store.cache_count();
            let mem = sys::process_memory_mb().map(|m| format!("{m} MB")).unwrap_or_else(|| "n/a".to_string());
            let disk =
                sys::free_space_mb(&app.cfg.temp_dir).map(|d| format!("{d} MB")).unwrap_or_else(|| "n/a".to_string());

            let text = format!(
                "📊 <b>System & Bot Status</b>\n\n\
                 • <b>Version:</b> <code>tg-media-bot {}</code>\n\
                 • <b>Uptime:</b> {}\n\
                 • <b>Active jobs:</b> {}\n\
                 • <b>Cache entries:</b> {}\n\
                 • <b>RAM (RSS):</b> {}\n\
                 • <b>Disk free:</b> {}\n\n\
                 ⚙️ <b>Limits & Config:</b>\n\
                 • Parallel downloads: {} (per user: {})\n\
                 • Max file size: {}\n\
                 • Max duration: {}\n\
                 • Timeout: {}\n\
                 • Allowed users: {}\n\
                 • Admins: {}",
                env!("CARGO_PKG_VERSION"),
                uptime,
                active,
                cache_entries,
                mem,
                disk,
                app.cfg.max_parallel,
                app.cfg.per_user,
                util::fmt_size(app.cfg.max_file_bytes),
                if app.cfg.max_duration > 0 { util::fmt_duration(app.cfg.max_duration) } else { "unlimited".into() },
                util::fmt_duration(app.cfg.download_timeout),
                if app.cfg.allowed_users.is_empty() {
                    "public".into()
                } else {
                    app.cfg.allowed_users.len().to_string()
                },
                if app.cfg.admin_users.is_empty() {
                    "allowed users".into()
                } else {
                    app.cfg.admin_users.len().to_string()
                },
            );
            app.say(chat, thread, &text, None).await;
        }
        "clean_cache" | "cleancache" => {
            if !app.is_admin(ctx.user) {
                app.say(chat, thread, t.not_authorized, None).await;
                return;
            }
            let count = app.store.clear_cache();
            app.say(chat, thread, &format!("🧹 Cache cleared: {count} entries removed."), None).await;
        }
        "broadcast" => {
            if !app.is_admin(ctx.user) {
                app.say(chat, thread, t.not_authorized, None).await;
                return;
            }
            if args.trim().is_empty() {
                app.say(chat, thread, "Usage: /broadcast &lt;message&gt;", None).await;
                return;
            }
            let chats = app.store.known_chats();
            let mut sent = 0usize;
            let mut failed = 0usize;
            for target_chat in chats {
                match app.bot.send_message(target_chat, None, args, None, None).await {
                    Ok(_) => sent += 1,
                    Err(_) => failed += 1,
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            app.say(chat, thread, &format!("📢 Broadcast completed: {sent} sent, {failed} failed."), None).await;
        }
        _ => {
            if !msg.chat.is_group() {
                app.say(chat, thread, t.nudge, None).await;
            }
        }
    }
}

async fn handle_callback(app: &Arc<App>, cb: CallbackQuery) {
    let data = cb.data.clone().unwrap_or_default();
    let Some(msg) = cb.message.as_ref() else {
        app.bot.answer_callback(&cb.id, None, false).await;
        return;
    };
    let lang = app.lang_for(msg.chat.id, Some(&cb.from));
    let t = lang.t();

    if !matches!(check_access(app, msg, Some(&cb.from), true), Access::Granted) {
        app.bot.answer_callback(&cb.id, Some(t.not_authorized), true).await;
        return;
    }

    let (prefix, rest) = data.split_once(':').unwrap_or((data.as_str(), ""));
    match prefix {
        "l" => {
            let Some(new) = Lang::from_code(rest) else {
                app.bot.answer_callback(&cb.id, None, false).await;
                return;
            };
            app.store.set_lang(msg.chat.id, new);
            app.bot.answer_callback(&cb.id, Some(new.button()), false).await;
            let text = fill(new.t().lang_set, &[("lang", new.button())]);
            if let Err(e) = app.bot.edit_message(msg.chat.id, msg.message_id, &text, None).await {
                log::debug!("edit after language change failed: {e}");
            }
            // Private chats also get the command menu in the chosen language.
            if !msg.chat.is_group() {
                let commands = command_list(new.t());
                if let Err(e) = app.bot.set_commands(&commands, None, Some(msg.chat.id)).await {
                    log::debug!("setMyCommands for chat failed: {e}");
                }
            }
        }
        // Answer to "which quality?".
        "q" => {
            let (token, code) = rest.split_once(':').unwrap_or((rest, ""));
            let Some(quality) = Quality::parse(code) else {
                app.bot.answer_callback(&cb.id, None, false).await;
                return;
            };
            let (urls, user_msg_id) = match app.take_urls(token, cb.from.id) {
                Ok(res) => res,
                Err(true) => {
                    app.bot.answer_callback(&cb.id, Some(t.not_yours), true).await;
                    return;
                }
                Err(false) => {
                    app.bot.answer_callback(&cb.id, Some(t.picker_expired), true).await;
                    return;
                }
            };
            app.bot.answer_callback(&cb.id, None, false).await;
            let ctx = Ctx { chat: msg.chat.id, thread: msg.thread(), user: cb.from.id, lang };
            enqueue_all(app, &ctx, urls, quality, Some(msg.message_id), user_msg_id).await;
        }
        // /quality: remember a default.
        "d" => {
            let choice = if rest == "ask" { None } else { Quality::parse(rest) };
            if rest != "ask" && choice.is_none() {
                app.bot.answer_callback(&cb.id, None, false).await;
                return;
            }
            app.store.set_default_quality(cb.from.id, choice);
            app.bot.answer_callback(&cb.id, None, false).await;
            let text = fill(t.quality_set, &[("q", &quality_name(t, choice))]);
            if let Err(e) = app.bot.edit_message(msg.chat.id, msg.message_id, &text, None).await {
                log::debug!("edit after quality change failed: {e}");
            }
        }
        "c" => {
            let is_status_list = msg.text.as_deref().is_some_and(|txt| txt.starts_with("📊"));
            if rest == "all" {
                let n = app.cancel_all(cb.from.id).await;
                let text = fill(t.cancelled_n, &[("n", &n.to_string())]);
                app.bot.answer_callback(&cb.id, Some(&text), false).await;
                if is_status_list {
                    let _ = app.bot.edit_message(msg.chat.id, msg.message_id, t.status_empty, None).await;
                }
                return;
            }
            match app.cancel(rest, cb.from.id).await {
                Cancel::Done => {
                    app.bot.answer_callback(&cb.id, Some(t.cancelled), false).await;
                    if is_status_list {
                        let remaining = app.user_jobs(cb.from.id);
                        if remaining.is_empty() {
                            let _ = app.bot.edit_message(msg.chat.id, msg.message_id, t.status_empty, None).await;
                        } else {
                            let (text, markup) = status_menu(app, cb.from.id, lang);
                            let _ = app.bot.edit_message(msg.chat.id, msg.message_id, &text, Some(markup)).await;
                        }
                    }
                }
                Cancel::Denied => app.bot.answer_callback(&cb.id, Some(t.cancel_denied), true).await,
                Cancel::NotFound => {
                    app.bot.answer_callback(&cb.id, None, false).await;
                    if is_status_list {
                        let remaining = app.user_jobs(cb.from.id);
                        if remaining.is_empty() {
                            let _ = app.bot.edit_message(msg.chat.id, msg.message_id, t.status_empty, None).await;
                        } else {
                            let (text, markup) = status_menu(app, cb.from.id, lang);
                            let _ = app.bot.edit_message(msg.chat.id, msg.message_id, &text, Some(markup)).await;
                        }
                    }
                }
            }
        }
        _ => app.bot.answer_callback(&cb.id, None, false).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(parse_command("/start", "mybot"), Some(("start".to_string(), "")));
        assert_eq!(
            parse_command("/Formats@MyBot  https://x.com ", "mybot"),
            Some(("formats".to_string(), "https://x.com"))
        );
        assert_eq!(parse_command("/start@otherbot", "mybot"), None);
        assert_eq!(parse_command("hello", "mybot"), None);
    }

    #[test]
    fn keyboards_fit_callback_limits() {
        for kb in
            [lang_keyboard(), picker_keyboard(Lang::Uk.t(), "ffffffabc", 1080), default_keyboard(Lang::Es.t(), 1080)]
        {
            for row in kb["inline_keyboard"].as_array().unwrap() {
                for b in row.as_array().unwrap() {
                    assert!(b["callback_data"].as_str().unwrap().len() <= 64);
                }
            }
        }
        let all = picker_keyboard(Lang::En.t(), "t", 480).to_string();
        assert!(all.contains("q:t:480") && all.contains("q:t:360") && all.contains("q:t:audio"));
        assert!(all.contains("q:t:max") && !all.contains("q:t:720"));
        assert!(default_keyboard(Lang::En.t(), 1080).to_string().contains("d:ask"));
        assert_eq!(command_list(Lang::Ru.t()).len(), 9);
        let lq = crate::app::lower_quality_keyboard(Lang::En.t(), "tok", Quality::Height(1080)).unwrap();
        let lq_str = lq.to_string();
        assert!(
            lq_str.contains("q:tok:720")
                && lq_str.contains("q:tok:480")
                && lq_str.contains("q:tok:360")
                && lq_str.contains("q:tok:audio")
        );
        assert!(crate::app::lower_quality_keyboard(Lang::En.t(), "tok", Quality::Audio).is_none());
    }

    #[tokio::test]
    async fn status_menu_renders_and_fits_limits() {
        let env = crate::config::Env::from_pairs(&[("BOT_TOKEN", "123:abc")]);
        let cfg = crate::config::Config::from_env(&env).unwrap();
        let bot = crate::telegram::Bot::new(&cfg).unwrap();
        let store = crate::state::Store::load(&cfg.data_dir);
        let plugins = crate::plugins::Plugins::load(None);
        let app = Arc::new(App::new(cfg, bot, store, plugins));

        let req = Request {
            chat_id: 10,
            thread_id: None,
            user_id: 42,
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            quality: Quality::Max,
            lang: Lang::Ru,
            status_msg: None,
            user_msg_id: None,
        };
        let job = app.enqueue(req).unwrap();
        job.set_title("Rick Astley - Never Gonna Give You Up");

        let (text, kb) = status_menu(&app, 42, Lang::Ru);
        assert!(text.contains("Rick Astley - Never Gonna Give You Up"));
        assert!(text.contains(&job.id));
        assert!(text.contains("📊 <b>Твоя очередь</b> (1/100)"));

        for row in kb["inline_keyboard"].as_array().unwrap() {
            for b in row.as_array().unwrap() {
                assert!(b["callback_data"].as_str().unwrap().len() <= 64);
                assert!(b["callback_data"].as_str().unwrap().starts_with("c:"));
            }
        }
    }
}
