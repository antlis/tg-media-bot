//! Small shared types.

use serde::{Deserialize, Serialize};

use crate::i18n::Strings;

/// What the user wants out of a link.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Video,
    Audio,
}

/// A quality choice: what the picker buttons and `/quality` offer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    /// Best that fits the limits.
    Max,
    /// At most this many pixels high.
    Height(u32),
    Audio,
}

impl Quality {
    pub const HEIGHTS: [u32; 4] = [1080, 720, 480, 360];

    pub fn code(self) -> String {
        match self {
            Quality::Max => "max".to_string(),
            Quality::Height(h) => h.to_string(),
            Quality::Audio => "audio".to_string(),
        }
    }

    pub fn parse(code: &str) -> Option<Quality> {
        match code {
            "max" => Some(Quality::Max),
            "audio" => Some(Quality::Audio),
            h => h.parse::<u32>().ok().filter(|h| (144..=4320).contains(h)).map(Quality::Height),
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Quality::Audio => Kind::Audio,
            _ => Kind::Video,
        }
    }

    pub fn cap(self) -> Option<u32> {
        match self {
            Quality::Height(h) => Some(h),
            _ => None,
        }
    }

    pub fn label(self, t: &Strings) -> String {
        match self {
            Quality::Max => t.btn_max.to_string(),
            Quality::Height(h) => format!("{h}p"),
            Quality::Audio => t.btn_audio.to_string(),
        }
    }
}

/// Where a job currently is; rendered into the status message.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Queued,
    Waiting,
    Probing,
    Downloading {
        pct: Option<f32>,
        /// bytes per second
        speed: Option<f64>,
        /// seconds left
        eta: Option<u64>,
    },
    Processing,
    Transcoding {
        pct: Option<f32>,
    },
    Uploading {
        total: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_codes() {
        for q in [Quality::Max, Quality::Audio, Quality::Height(720)] {
            assert_eq!(Quality::parse(&q.code()), Some(q));
        }
        assert_eq!(Quality::parse("99999"), None);
        assert_eq!(Quality::parse("ask"), None);
        assert_eq!(Quality::Height(480).cap(), Some(480));
        assert_eq!(Quality::Audio.kind(), Kind::Audio);
    }
}
