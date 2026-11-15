//! Localization. Every language is a `static Strings`; a missing key is a
//! compile error, so translations cannot silently fall out of sync.
//!
//! To add a language: copy `en.rs`, translate it, add a `Lang` variant and
//! wire it up in the four `match`es below.

use serde::{Deserialize, Serialize};

mod en;
mod es;
mod ru;
mod uk;

macro_rules! strings {
    ($($name:ident),* $(,)?) => {
        pub struct Strings { $(pub $name: &'static str,)* }

        impl Strings {
            #[cfg(test)]
            pub fn fields(&self) -> Vec<(&'static str, &'static str)> {
                vec![$((stringify!($name), self.$name),)*]
            }
        }
    };
}

strings! {
    // general
    welcome, help, help_limits, help_limit_duration, lang_set, nudge, not_authorized, cleared,
    // /quality
    quality_current, quality_set, word_ask,
    // /cancel, /status
    cancel_usage, cancelled, cancelled_n, cancel_fail, cancel_denied, status_empty, status_header, status_more,
    // /minimal
    minimal_status, minimal_on, minimal_off, word_on, word_off,
    // /topic
    topic_groups_only, topic_lock_outside, topic_locked, topic_unlocked, topic_none, topic_status, topic_usage,
    // quality picker
    formats_usage, ask_quality, ask_quality_many, queued_n, picker_expired, not_yours,
    btn_max, btn_audio, btn_ask, btn_cancel, btn_cancel_all,
    // queueing
    invalid_url, private_url, user_queue_full, queue_partial, queue_full, duplicate,
    // stages
    st_queued, st_waiting, st_probing, st_downloading, st_processing, st_transcoding, st_uploading,
    st_title_pending, st_resuming,
    // notices & errors
    restarted_notice,
    err_title, e_age, e_bot, e_private, e_members, e_login, e_format, e_unavailable, e_unsupported,
    e_no_media, e_404, e_403, e_rate, e_network, e_geo, e_drm, e_live, e_too_long, e_too_large,
    e_no_space, e_timeout, e_probe_timeout, e_upload, e_upload_big, e_internal,
    // command menu
    c_start, c_help, c_quality, c_status, c_cancel, c_clear, c_minimal, c_topic, c_lang,
}

/// Shown above the language buttons; deliberately multilingual.
pub const LANG_PROMPT: &str = "🌐 Language · Язык · Мова · Idioma";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ru,
    Uk,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::En, Lang::Ru, Lang::Uk, Lang::Es];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Uk => "uk",
            Lang::Es => "es",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim().to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Best match for a Telegram `language_code` such as `ru` or `es-419`.
    pub fn guess(language_code: Option<&str>) -> Option<Lang> {
        let code = language_code?.to_ascii_lowercase();
        let primary = code.split(['-', '_']).next()?;
        match primary {
            "be" | "kk" => Some(Lang::Ru),
            other => Lang::from_code(other),
        }
    }

    /// Label for the language picker button.
    pub fn button(self) -> &'static str {
        match self {
            Lang::En => "🇬🇧 English",
            Lang::Ru => "🇷🇺 Русский",
            Lang::Uk => "🇺🇦 Українська",
            Lang::Es => "🇪🇸 Español",
        }
    }

    pub fn t(self) -> &'static Strings {
        match self {
            Lang::En => &en::STRINGS,
            Lang::Ru => &ru::STRINGS,
            Lang::Uk => &uk::STRINGS,
            Lang::Es => &es::STRINGS,
        }
    }
}

/// Replace `{name}` placeholders in a template.
pub fn fill(tpl: &str, args: &[(&str, &str)]) -> String {
    let mut out = tpl.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<String> {
        let mut found: Vec<String> =
            s.split('{').skip(1).filter_map(|part| part.split_once('}').map(|(name, _)| name.to_string())).collect();
        found.sort();
        found
    }

    #[test]
    fn translations_are_consistent() {
        let en = Lang::En.t().fields();
        for lang in Lang::ALL {
            for ((key, text), (_, en_text)) in lang.t().fields().into_iter().zip(&en) {
                assert!(!text.trim().is_empty(), "{lang:?}.{key} is empty");
                assert_eq!(placeholders(text), placeholders(en_text), "{lang:?}.{key}: placeholders differ");
                for tag in ["b", "i", "code"] {
                    let open = text.matches(&format!("<{tag}>")).count();
                    let close = text.matches(&format!("</{tag}>")).count();
                    assert_eq!(open, close, "{lang:?}.{key}: unbalanced <{tag}>");
                }
                // Everything is sent with parse_mode=HTML: a stray '<' breaks the message.
                let stripped = ["<b>", "</b>", "<i>", "</i>", "<code>", "</code>"]
                    .iter()
                    .fold(text.to_string(), |acc, tag| acc.replace(tag, ""));
                assert!(!stripped.contains('<') && !stripped.contains('>'), "{lang:?}.{key}: raw angle bracket");
            }
        }
    }

    #[test]
    fn command_descriptions_fit_telegram_limits() {
        for lang in Lang::ALL {
            for (key, text) in lang.t().fields() {
                if key.starts_with("c_") {
                    let n = text.chars().count();
                    assert!((1..=256).contains(&n), "{lang:?}.{key}: bad length");
                }
                if key.starts_with("btn_") {
                    assert!(text.chars().count() <= 32, "{lang:?}.{key}: button too long");
                }
            }
        }
    }

    #[test]
    fn codes_and_guessing() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::guess(Some("ru")), Some(Lang::Ru));
        assert_eq!(Lang::guess(Some("es-419")), Some(Lang::Es));
        assert_eq!(Lang::guess(Some("pt-BR")), None);
        assert_eq!(Lang::guess(None), None);
        assert_eq!(fill("a {x} b {y}", &[("x", "1"), ("y", "2")]), "a 1 b 2");
    }
}
