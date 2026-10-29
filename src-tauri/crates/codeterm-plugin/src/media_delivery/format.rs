use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaDeliveryDescriptor {
    pub types: Vec<MediaType>,
}
impl MediaDeliveryDescriptor {
    pub fn valid(&self) -> bool {
        !self.types.is_empty()
            && self.types.len() <= 10
            && self
                .types
                .iter()
                .enumerate()
                .all(|(i, t)| !self.types[..i].contains(t))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MediaFile {
    pub path: String,
    pub content_type: MediaType,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    #[serde(rename = "video/mp4")]
    Mp4,
    #[serde(rename = "video/webm")]
    Webm,
    #[serde(rename = "video/ogg")]
    OggVideo,
    #[serde(rename = "audio/mpeg")]
    Mp3,
    #[serde(rename = "audio/ogg")]
    OggAudio,
    #[serde(rename = "audio/wav")]
    Wav,
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    Webp,
    #[serde(rename = "image/gif")]
    Gif,
}
impl MediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp4 => "video/mp4",
            Self::Webm => "video/webm",
            Self::OggVideo => "video/ogg",
            Self::Mp3 => "audio/mpeg",
            Self::OggAudio => "audio/ogg",
            Self::Wav => "audio/wav",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
        }
    }
    pub fn image(self)->bool {matches!(self,Self::Png|Self::Jpeg|Self::Webp|Self::Gif)}
    pub fn playback(self) -> bool {
        self.as_str().starts_with("audio/") || self.as_str().starts_with("video/")
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::OggVideo | Self::OggAudio => "ogg",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
        }
    }
    pub fn matches(self, head: &[u8]) -> bool {
        match self {
            Self::Mp4 => head.get(4..8) == Some(b"ftyp"),
            Self::Webm => head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
            Self::OggVideo | Self::OggAudio => head.starts_with(b"OggS"),
            Self::Mp3 => {
                head.starts_with(b"ID3")
                    || (head.len() >= 2 && head[0] == 0xff && head[1] & 0xe0 == 0xe0)
            }
            Self::Wav => head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE"),
            Self::Png => head.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpeg => head.starts_with(&[0xff, 0xd8, 0xff]),
            Self::Webp => head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP"),
            Self::Gif => head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a"),
        }
    }
}
pub(super) fn safe_name(name: &str, kind: MediaType) -> String {
    safe_extension_name(name, kind.extension())
}

pub(super) fn safe_extension_name(name: &str, extension: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let cleaned: String = stem
        .chars()
        .take(100)
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches([' ', '.']);
    let prefix = cleaned
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let device_suffix = prefix
        .strip_prefix("COM")
        .or_else(|| prefix.strip_prefix("LPT"));
    let reserved = matches!(prefix.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device_suffix.is_some_and(|suffix| {
            let mut chars = suffix.chars();
            matches!(chars.next(), Some('1'..='9' | '¹' | '²' | '³')) && chars.next().is_none()
        });
    format!(
        "{}.{}",
        if cleaned.is_empty() || reserved {
            "media"
        } else {
            cleaned
        },
        extension
    )
}
