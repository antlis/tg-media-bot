use super::Strings;

pub static STRINGS: Strings = Strings {
    welcome: "👋 <b>Hi!</b> Send me links to videos or tracks — I'll ask for the quality and send the files back.\n\n/help — commands\n/lang — language",
    help: "<b>Media downloader</b>\n\n\
Send links (several per message is fine). I'll ask for the quality, put them in your queue and send the files one by one.\n\n\
<b>Commands</b>\n\
/quality — default quality (or ask every time)\n\
/status — your queue\n\
/cancel &lt;id&gt; — cancel a download, /cancel all — clear your queue\n\
/minimal on|off — no status messages, no captions\n\
/topic lock|unlock|status — keep the bot in one forum topic\n\
/lang — language\n\n\
<b>Sites</b>\n\
YouTube, TikTok, Instagram, X/Twitter, Reddit, Vimeo, Twitch, SoundCloud and 1000+ more (yt-dlp).",
    help_limits: "<b>Limits</b>\nFile up to {size} · video up to {height}p",
    help_limit_duration: " · up to {dur} long",
    lang_set: "✅ Language: {lang}",
    nudge: "👋 Send me a link to download.\n/help — commands",
    not_authorized: "⛔ You are not allowed to use this bot.",
    cleared: "🧹 Chat cleared.",

    quality_current: "🎚 Quality for new links: <b>{q}</b>\nPick what to use from now on:",
    quality_set: "✅ New links will be downloaded as: <b>{q}</b>. Change it with /quality.",
    word_ask: "ask every time",

    cancel_usage: "Usage: /cancel &lt;id&gt; or /cancel all\nSee /status for ids.",
    cancelled: "🚫 Cancelled.",
    cancelled_n: "🚫 Downloads cancelled: {n}",
    cancel_fail: "❌ No active download with id <code>{id}</code>.",
    cancel_denied: "Only the person who started this download can cancel it.",
    status_empty: "Your queue is empty.",
    status_header: "📊 <b>Your queue</b> ({n}/{max})",
    status_more: "…and {n} more",

    minimal_status: "Minimal mode is <b>{state}</b> for this chat.\nUsage: /minimal on|off",
    minimal_on: "🤫 Minimal mode on: no status messages, no captions.",
    minimal_off: "✅ Minimal mode off.",
    word_on: "on",
    word_off: "off",

    topic_groups_only: "Topic lock only works in groups with topics.",
    topic_lock_outside: "Send /topic lock inside the topic you want me to stay in.",
    topic_locked: "🔒 From now on I only respond in this topic. /topic unlock to undo.",
    topic_unlocked: "🔓 Topic lock removed — I respond in every topic again.",
    topic_none: "No topic lock in this group.",
    topic_status: "🔒 Locked to topic {id}.",
    topic_usage: "Usage: /topic lock|unlock|status",

    formats_usage: "Usage: /formats &lt;link&gt;",
    ask_quality: "🎚 Which quality?",
    ask_quality_many: "🎚 Which quality? Links: {n}",
    queued_n: "✅ {choice} · queued: {n}",
    picker_expired: "This menu has expired — send the link again.",
    not_yours: "These buttons are for the person who sent the links.",
    btn_max: "🎬 Best",
    btn_audio: "🎵 Audio",
    btn_ask: "❓ Ask every time",
    btn_cancel: "✖ Cancel",
    btn_cancel_all: "✖ Cancel all",

    invalid_url: "❌ That doesn't look like a valid link.",
    private_url: "⛔ Links to local or private addresses are not allowed.",
    user_queue_full: "⏳ Your queue is full: {n} of {max}. Wait for some downloads to finish or /cancel all.",
    queue_partial: "⚠️ Queued {n}; {skipped} did not fit (your queue holds {max}).",
    queue_full: "⏳ The bot's queue is full right now — try again later.",
    duplicate: "⏳ This link is already in the queue.",

    st_queued: "⏳ Queued",
    st_waiting: "⏳ Waiting for a free slot",
    st_probing: "🔎 Checking the link",
    st_downloading: "📥 Downloading",
    st_processing: "🔄 Processing",
    st_transcoding: "🎞 Converting",
    st_uploading: "📤 Uploading",
    st_title_pending: "fetching title…",
    st_resuming: "🔄 Resuming download after bot restart…",

    restarted_notice: "⚠️ Bot was restarted. Please send the link again.",
    err_title: "❌ <b>Download failed</b>",
    e_age: "🔞 Age-restricted. The bot owner has to set COOKIES_FILE from a logged-in account.",
    e_bot: "🤖 The site wants a sign-in to prove you're not a bot. The bot owner has to set COOKIES_FILE.",
    e_private: "🔒 This video is private.",
    e_members: "🔒 Members-only content.",
    e_login: "🔑 Login required. The bot owner has to set COOKIES_FILE from a logged-in account.",
    e_format: "🎚 That quality isn't available — try another one.",
    e_unavailable: "🚫 The video is unavailable (removed or not public).",
    e_unsupported: "🤔 This link isn't supported.",
    e_no_media: "🤔 No video or audio found at this link.",
    e_404: "🚫 Not found (404) — the link may be dead.",
    e_403: "⛔ Access denied (403) — may need cookies or another region.",
    e_rate: "⏳ The site is rate-limiting requests — try again later.",
    e_network: "🌐 Couldn't reach the site — try again in a bit.",
    e_geo: "🌍 Not available in the server's region (the bot owner can set PROXY_URL).",
    e_drm: "🔐 This media is DRM-protected and can't be downloaded.",
    e_live: "🔴 Live streams aren't supported — try again once the stream has ended.",
    e_too_long: "⏱ Too long: {dur} (limit {max}).",
    e_too_large: "📦 Too large: about {size} (limit {max}).\nTry a lower quality or audio.",
    e_no_space: "💾 The server is low on disk space — try again later.",
    e_timeout: "⏱ Timed out after {min} min.",
    e_probe_timeout: "⏱ The source site took too long to respond — possible network or connectivity issue.",
    e_upload: "❌ Telegram didn't accept the file ({size}).",
    e_upload_big: "📦 The file is {size} — over the {max} upload limit.",
    e_internal: "❌ Internal error. Details are in the bot's log.",

    c_start: "Start the bot",
    c_help: "Help and supported sites",
    c_quality: "Default quality",
    c_status: "Your queue",
    c_cancel: "Cancel a download (or all)",
    c_clear: "Clear message history: [count]",
    c_minimal: "Minimal UI: on|off",
    c_topic: "Stay in one topic: lock|unlock|status",
    c_lang: "Language",
};
