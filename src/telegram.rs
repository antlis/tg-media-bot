//! Thin Telegram Bot API client: just the handful of methods the bot needs.
//! Files are streamed from disk, never buffered in memory.

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::TryStreamExt;
use reqwest::multipart::{Form, Part};
use serde::de::{DeserializeOwned, IgnoredAny};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::io::ReaderStream;

use crate::config::Config;

const SHORT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum TgError {
    Api { code: i64, desc: String, retry_after: Option<u64> },
    Net(String),
}

impl TgError {
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            TgError::Api { retry_after, .. } => *retry_after,
            TgError::Net(_) => None,
        }
    }

    pub fn code(&self) -> Option<i64> {
        match self {
            TgError::Api { code, .. } => Some(*code),
            TgError::Net(_) => None,
        }
    }
}

impl fmt::Display for TgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TgError::Api { code, desc, .. } => write!(f, "Telegram API {code}: {desc}"),
            TgError::Net(e) => write!(f, "network: {e}"),
        }
    }
}

/// reqwest errors embed the request URL, which contains the bot token.
fn net(e: reqwest::Error) -> TgError {
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    TgError::Net(msg)
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    error_code: Option<i64>,
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    retry_after: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    pub username: Option<String>,
    pub language_code: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type", default)]
    pub kind: String,
}

impl Chat {
    pub fn is_group(&self) -> bool {
        self.kind == "group" || self.kind == "supergroup"
    }
}

#[derive(Deserialize, Debug)]
pub struct Entity {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct FileRef {
    pub file_id: String,
}

#[derive(Deserialize, Debug)]
pub struct Message {
    pub message_id: i64,
    pub from: Option<User>,
    pub chat: Chat,
    pub text: Option<String>,
    pub caption: Option<String>,
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub caption_entities: Vec<Entity>,
    pub message_thread_id: Option<i64>,
    #[serde(default)]
    pub is_topic_message: bool,
    pub reply_to_message: Option<Box<Message>>,
    pub video: Option<FileRef>,
    pub audio: Option<FileRef>,
    pub document: Option<FileRef>,
}

impl Message {
    /// Forum topic this message belongs to (None for "General" / non-forums).
    pub fn thread(&self) -> Option<i64> {
        if self.is_topic_message {
            self.message_thread_id
        } else {
            None
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub message: Option<Message>,
    pub data: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Update {
    pub message: Option<Message>,
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Video,
    Audio,
    Document,
}

impl MediaKind {
    fn method(self) -> &'static str {
        match self {
            MediaKind::Video => "sendVideo",
            MediaKind::Audio => "sendAudio",
            MediaKind::Document => "sendDocument",
        }
    }

    fn field(self) -> &'static str {
        match self {
            MediaKind::Video => "video",
            MediaKind::Audio => "audio",
            MediaKind::Document => "document",
        }
    }
}

pub enum Source<'a> {
    /// Re-send something Telegram already has.
    FileId(&'a str),
    /// Stream the file over HTTP.
    Upload(&'a Path),
    /// Local Bot API server only: let the server read the file from disk.
    LocalUri(&'a Path),
}

pub struct Media<'a> {
    pub kind: MediaKind,
    pub chat_id: i64,
    pub thread_id: Option<i64>,
    pub source: Source<'a>,
    pub file_name: &'a str,
    pub caption: Option<&'a str>,
    pub duration: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub title: Option<&'a str>,
    pub performer: Option<&'a str>,
    pub thumb: Option<&'a Path>,
}

#[derive(Clone)]
pub struct Bot {
    http: reqwest::Client,
    base: String,
    pub id: i64,
    pub username: String,
}

impl Bot {
    pub fn new(cfg: &Config) -> Result<Bot, String> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(16)
            .tcp_keepalive(Duration::from_secs(60))
            .tcp_nodelay(true)
            .user_agent(concat!("tg-media-bot/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Bot { http, base: format!("{}/bot{}", cfg.api_url, cfg.bot_token), id: 0, username: String::new() })
    }

    async fn finish<T: DeserializeOwned>(resp: Result<reqwest::Response, reqwest::Error>) -> Result<T, TgError> {
        let bytes = resp.map_err(net)?.bytes().await.map_err(net)?;
        let env: Envelope<T> =
            serde_json::from_slice(&bytes).map_err(|e| TgError::Net(format!("unexpected response: {e}")))?;
        if env.ok {
            env.result.ok_or_else(|| TgError::Net("response without result".into()))
        } else {
            Err(TgError::Api {
                code: env.error_code.unwrap_or(0),
                desc: env.description.unwrap_or_default(),
                retry_after: env.parameters.and_then(|p| p.retry_after),
            })
        }
    }

    async fn call<T: DeserializeOwned>(&self, method: &str, body: &Value, timeout: Duration) -> Result<T, TgError> {
        let req = self.http.post(format!("{}/{method}", self.base)).json(body).timeout(timeout);
        Self::finish(req.send().await).await
    }

    /// Fetch our own identity; must be called once before polling.
    pub async fn init(&mut self) -> Result<(), TgError> {
        let me: User = self.call("getMe", &json!({}), SHORT_TIMEOUT).await?;
        self.id = me.id;
        self.username = me.username.unwrap_or_default();
        Ok(())
    }

    /// Long-poll for updates. Returns `(next_offset, updates)`; updates that
    /// fail to parse are skipped rather than wedging the queue.
    pub async fn get_updates(&self, offset: i64, wait_secs: u64) -> Result<(i64, Vec<Update>), TgError> {
        let body = json!({
            "offset": offset,
            "timeout": wait_secs,
            "limit": 50,
            "allowed_updates": ["message", "callback_query"],
        });
        let raw: Vec<Value> = self.call("getUpdates", &body, Duration::from_secs(wait_secs + 15)).await?;
        let mut next = offset;
        let mut updates = Vec::with_capacity(raw.len());
        for value in raw {
            if let Some(id) = value.get("update_id").and_then(Value::as_i64) {
                next = next.max(id + 1);
            }
            match serde_json::from_value::<Update>(value) {
                Ok(u) => updates.push(u),
                Err(e) => log::warn!("skipping unparsable update: {e}"),
            }
        }
        Ok((next, updates))
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        text: &str,
        markup: Option<Value>,
        reply_to: Option<i64>,
    ) -> Result<Message, TgError> {
        let mut body = json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
        });
        if let Some(t) = thread_id {
            body["message_thread_id"] = json!(t);
        }
        if let Some(m) = markup {
            body["reply_markup"] = m;
        }
        if let Some(r) = reply_to {
            body["reply_parameters"] = json!({
                "message_id": r,
                "allow_sending_without_reply": true,
            });
        }
        self.call("sendMessage", &body, SHORT_TIMEOUT).await
    }

    /// Edit a message's text; passing no markup removes any inline keyboard.
    pub async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        markup: Option<Value>,
    ) -> Result<(), TgError> {
        let mut body = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
        });
        if let Some(m) = markup {
            body["reply_markup"] = m;
        }
        match self.call::<IgnoredAny>("editMessageText", &body, SHORT_TIMEOUT).await {
            Ok(_) => Ok(()),
            // Same text as before — not an error for our purposes.
            Err(TgError::Api { desc, .. }) if desc.contains("message is not modified") => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), TgError> {
        let body = json!({ "chat_id": chat_id, "message_id": message_id });
        self.call::<IgnoredAny>("deleteMessage", &body, SHORT_TIMEOUT).await.map(|_| ())
    }

    pub async fn send_chat_action(&self, chat_id: i64, action: &str, thread_id: Option<i64>) -> Result<(), TgError> {
        let mut body = json!({
            "chat_id": chat_id,
            "action": action,
        });
        if let Some(t) = thread_id {
            body["message_thread_id"] = json!(t);
        }
        self.call::<IgnoredAny>("sendChatAction", &body, Duration::from_secs(5)).await.map(|_| ())
    }

    pub async fn delete_messages(&self, chat_id: i64, message_ids: &[i64]) -> Result<(), TgError> {
        if message_ids.is_empty() {
            return Ok(());
        }
        let body = json!({ "chat_id": chat_id, "message_ids": message_ids });
        match self.call::<IgnoredAny>("deleteMessages", &body, SHORT_TIMEOUT).await {
            Ok(_) => Ok(()),
            Err(e) => {
                log::debug!("deleteMessages failed ({e}), falling back to individual deleteMessage");
                for &id in message_ids {
                    let _ = self.delete_message(chat_id, id).await;
                }
                Ok(())
            }
        }
    }

    pub async fn answer_callback(&self, id: &str, text: Option<&str>, alert: bool) {
        let mut body = json!({ "callback_query_id": id });
        if let Some(t) = text {
            body["text"] = json!(t);
            body["show_alert"] = json!(alert);
        }
        if let Err(e) = self.call::<IgnoredAny>("answerCallbackQuery", &body, SHORT_TIMEOUT).await {
            log::debug!("answerCallbackQuery: {e}");
        }
    }

    /// `commands` is a list of `(command, description)`. `language` scopes the
    /// list to users with that Telegram UI language; `chat_id` to one chat.
    pub async fn set_commands(
        &self,
        commands: &[(&str, &str)],
        language: Option<&str>,
        chat_id: Option<i64>,
    ) -> Result<(), TgError> {
        let list: Vec<Value> = commands.iter().map(|(c, d)| json!({ "command": c, "description": d })).collect();
        let mut body = json!({ "commands": list });
        if let Some(l) = language {
            body["language_code"] = json!(l);
        }
        if let Some(c) = chat_id {
            body["scope"] = json!({ "type": "chat", "chat_id": c });
        }
        self.call::<IgnoredAny>("setMyCommands", &body, SHORT_TIMEOUT).await.map(|_| ())
    }

    /// Send a video/audio/document. `sent` (if given) is advanced as bytes of
    /// the main file leave the process, for upload progress.
    pub async fn send_media(&self, m: &Media<'_>, sent: Option<Arc<AtomicU64>>) -> Result<Message, TgError> {
        let mut form = Form::new().text("chat_id", m.chat_id.to_string());
        if let Some(t) = m.thread_id {
            form = form.text("message_thread_id", t.to_string());
        }
        if let Some(c) = m.caption {
            form = form.text("caption", c.to_string()).text("parse_mode", "HTML");
        }
        if let Some(d) = m.duration.filter(|_| m.kind != MediaKind::Document) {
            form = form.text("duration", d.to_string());
        }
        if m.kind == MediaKind::Video {
            form = form.text("supports_streaming", "true");
            if let (Some(w), Some(h)) = (m.width, m.height) {
                form = form.text("width", w.to_string()).text("height", h.to_string());
            }
        }
        if m.kind == MediaKind::Audio {
            if let Some(t) = m.title {
                form = form.text("title", t.to_string());
            }
            if let Some(p) = m.performer {
                form = form.text("performer", p.to_string());
            }
        }

        let mut timeout = Duration::from_secs(120);
        match m.source {
            Source::FileId(id) => form = form.text(m.kind.field(), id.to_string()),
            Source::LocalUri(path) => {
                form = form.text(m.kind.field(), file_uri(path));
                if let Some(thumb) = m.thumb {
                    form = form.text("thumbnail", file_uri(thumb));
                }
                timeout = Duration::from_secs(900);
            }
            Source::Upload(path) => {
                let (part, len) = file_part(path, m.file_name, sent).await?;
                form = form.part(m.kind.field(), part);
                if let Some(thumb) = m.thumb {
                    if let Ok((tpart, _)) = file_part(thumb, "thumb.jpg", None).await {
                        form = form.part("thumbnail", tpart);
                    }
                }
                // Generous floor of ~64 KB/s before giving up on an upload.
                timeout = Duration::from_secs((120 + len / 65_536).min(4 * 3600));
            }
        }

        let req = self.http.post(format!("{}/{}", self.base, m.kind.method())).multipart(form).timeout(timeout);
        Self::finish(req.send().await).await
    }
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

async fn file_part(path: &Path, name: &str, sent: Option<Arc<AtomicU64>>) -> Result<(Part, u64), TgError> {
    let io = |e: std::io::Error| TgError::Net(format!("{}: {e}", path.display()));
    let file = tokio::fs::File::open(path).await.map_err(io)?;
    let len = file.metadata().await.map_err(io)?.len();
    let stream = ReaderStream::with_capacity(file, 512 * 1024).inspect_ok(move |chunk| {
        if let Some(counter) = &sent {
            counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        }
    });
    let part = Part::stream_with_length(reqwest::Body::wrap_stream(stream), len).file_name(name.to_string());
    Ok((part, len))
}
