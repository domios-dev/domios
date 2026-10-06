use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Text,
    Audio,
    Video,
    Image,
}

impl MediaKind {
    pub const ALL: [MediaKind; 4] = [
        MediaKind::Text,
        MediaKind::Audio,
        MediaKind::Video,
        MediaKind::Image,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MediaKind::Text => "text",
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
            MediaKind::Image => "image",
        }
    }

    pub fn parse(value: &str) -> Option<MediaKind> {
        MediaKind::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::MediaKind;

    #[test]
    fn media_kind_round_trips_through_serde_and_parse() {
        for kind in MediaKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(serde_json::from_str::<MediaKind>(&json).unwrap(), kind);
            assert_eq!(MediaKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(MediaKind::parse("Audio"), None);
        assert!(serde_json::from_str::<MediaKind>("\"midi\"").is_err());
    }
}
