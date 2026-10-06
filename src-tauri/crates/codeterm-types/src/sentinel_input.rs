use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// Upper bound on inline text carried inside a definition.
pub const MAX_INLINE_TEXT_BYTES: usize = 64 * 1024;
/// Runtime bound on a file or fetched source; never checked at definition time.
pub const MAX_INPUT_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
/// Runtime bound on converted text produced by the converter capability.
pub const MAX_CONVERTED_TEXT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_CONVERT_TIMEOUT_MS: u64 = 15 * 60 * 1000;
pub const MAX_CONVERT_TIMEOUT_MS: u64 = 60 * 60 * 1000;
pub const MAX_URL_BYTES: usize = 2048;

/// Where an input stage's data lives; `Text` is allowed only for `MediaKind::Text`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataRef {
    Text(String),
    File(PathBuf),
    Url(String),
}

/// Immutable input kind plus the amendable data reference and conversion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpec {
    pub kind: MediaKind,
    pub data: DataRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convert_to: Option<MediaKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convert_timeout_ms: Option<u64>,
}

/// One distinct refusal per broken rule, so callers can match the exact failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputError {
    InlineTextNotAllowed,
    InlineTextTooLarge,
    InlineTextContainsNul,
    EmptyPath,
    RelativePath,
    UrlTooLong,
    InvalidUrl,
    UnsupportedUrlScheme,
    TextCannotConvert,
    ConvertToEqualsKind,
    ConvertTimeoutNotPositive,
    ConvertTimeoutAboveMax,
}

impl InputError {
    pub fn as_str(self) -> &'static str {
        match self {
            InputError::InlineTextNotAllowed => "inline_text_not_allowed",
            InputError::InlineTextTooLarge => "inline_text_too_large",
            InputError::InlineTextContainsNul => "inline_text_contains_nul",
            InputError::EmptyPath => "empty_path",
            InputError::RelativePath => "relative_path",
            InputError::UrlTooLong => "url_too_long",
            InputError::InvalidUrl => "invalid_url",
            InputError::UnsupportedUrlScheme => "unsupported_url_scheme",
            InputError::TextCannotConvert => "text_cannot_convert",
            InputError::ConvertToEqualsKind => "convert_to_equals_kind",
            InputError::ConvertTimeoutNotPositive => "convert_timeout_not_positive",
            InputError::ConvertTimeoutAboveMax => "convert_timeout_above_max",
        }
    }
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Definition-time rules only; file existence and source size are runtime checks.
pub fn validate(spec: &InputSpec) -> Result<(), InputError> {
    match &spec.data {
        DataRef::Text(text) => {
            if spec.kind != MediaKind::Text {
                return Err(InputError::InlineTextNotAllowed);
            }
            if text.len() > MAX_INLINE_TEXT_BYTES {
                return Err(InputError::InlineTextTooLarge);
            }
            if text.contains('\0') {
                return Err(InputError::InlineTextContainsNul);
            }
        }
        DataRef::File(path) => {
            if path.as_os_str().is_empty() {
                return Err(InputError::EmptyPath);
            }
            if !path.is_absolute() {
                return Err(InputError::RelativePath);
            }
        }
        DataRef::Url(url) => {
            if url.len() > MAX_URL_BYTES {
                return Err(InputError::UrlTooLong);
            }
            let Some((scheme, rest)) = url.split_once("://") else {
                return Err(InputError::InvalidUrl);
            };
            if scheme.is_empty() {
                return Err(InputError::InvalidUrl);
            }
            if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
                return Err(InputError::UnsupportedUrlScheme);
            }
            if url_host(rest).is_empty() {
                return Err(InputError::InvalidUrl);
            }
        }
    }
    if spec.kind == MediaKind::Text && spec.convert_to.is_some() {
        return Err(InputError::TextCannotConvert);
    }
    if spec.convert_to == Some(spec.kind) {
        return Err(InputError::ConvertToEqualsKind);
    }
    if let Some(timeout) = spec.convert_timeout_ms {
        if timeout == 0 {
            return Err(InputError::ConvertTimeoutNotPositive);
        }
        if timeout > MAX_CONVERT_TIMEOUT_MS {
            return Err(InputError::ConvertTimeoutAboveMax);
        }
    }
    Ok(())
}

/// Safe display identity of a source: a file name or URL host, never inline text.
pub fn display_name(data: &DataRef) -> String {
    match data {
        DataRef::Text(_) => String::new(),
        DataRef::File(path) => file_name(path),
        DataRef::Url(url) => url
            .split_once("://")
            .map(|(_, rest)| url_host(rest))
            .unwrap_or_default(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn url_host(rest: &str) -> String {
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    if let Some(ipv6) = host.strip_prefix('[') {
        return ipv6.split(']').next().unwrap_or_default().to_string();
    }
    host.split(':').next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn absolute(name: &str) -> PathBuf {
        std::env::current_dir().unwrap().join(name)
    }

    fn spec(kind: MediaKind, data: DataRef) -> InputSpec {
        InputSpec {
            kind,
            data,
            convert_to: None,
            convert_timeout_ms: None,
        }
    }

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

    #[test]
    fn validate_accepts_the_valid_matrix() {
        let inline = spec(MediaKind::Text, DataRef::Text("hello".into()));
        assert_eq!(validate(&inline), Ok(()));

        let cases = vec![
            spec(MediaKind::Text, DataRef::File(absolute("notes.txt"))),
            spec(MediaKind::Text, DataRef::Url("https://example.com/a".into())),
            spec(MediaKind::Audio, DataRef::File(absolute("voice.wav"))),
            spec(MediaKind::Audio, DataRef::Url("http://example.com:8080/a?b=1".into())),
            spec(MediaKind::Video, DataRef::File(absolute("clip.mp4"))),
            spec(MediaKind::Video, DataRef::Url("https://example.com/clip".into())),
            spec(MediaKind::Image, DataRef::File(absolute("shot.png"))),
            spec(MediaKind::Image, DataRef::Url("https://example.com/shot.png".into())),
        ];
        for case in &cases {
            assert_eq!(validate(case), Ok(()), "{:?}", case);
        }

        let mut converted = spec(MediaKind::Audio, DataRef::File(absolute("voice.wav")));
        converted.convert_to = Some(MediaKind::Text);
        converted.convert_timeout_ms = Some(DEFAULT_CONVERT_TIMEOUT_MS);
        assert_eq!(validate(&converted), Ok(()));
        converted.convert_timeout_ms = Some(MAX_CONVERT_TIMEOUT_MS);
        assert_eq!(validate(&converted), Ok(()));
    }

    #[test]
    fn validate_refuses_each_broken_rule_with_its_own_variant() {
        let oversized = "x".repeat(MAX_INLINE_TEXT_BYTES + 1);
        let long_url = format!("https://example.com/{}", "u".repeat(MAX_URL_BYTES));
        let cases: Vec<(InputError, InputSpec)> = vec![
            (
                InputError::InlineTextNotAllowed,
                spec(MediaKind::Audio, DataRef::Text("hello".into())),
            ),
            (
                InputError::InlineTextNotAllowed,
                spec(MediaKind::Image, DataRef::Text(String::new())),
            ),
            (
                InputError::InlineTextTooLarge,
                spec(MediaKind::Text, DataRef::Text(oversized)),
            ),
            (
                InputError::InlineTextContainsNul,
                spec(MediaKind::Text, DataRef::Text("a\0b".into())),
            ),
            (
                InputError::EmptyPath,
                spec(MediaKind::Audio, DataRef::File(PathBuf::new())),
            ),
            (
                InputError::RelativePath,
                spec(MediaKind::Audio, DataRef::File(PathBuf::from("voice.wav"))),
            ),
            (
                InputError::UrlTooLong,
                spec(MediaKind::Audio, DataRef::Url(long_url)),
            ),
            (
                InputError::InvalidUrl,
                spec(MediaKind::Audio, DataRef::Url("example.com/a".into())),
            ),
            (
                InputError::InvalidUrl,
                spec(MediaKind::Audio, DataRef::Url("https://".into())),
            ),
            (
                InputError::UnsupportedUrlScheme,
                spec(MediaKind::Audio, DataRef::Url("ftp://example.com/a".into())),
            ),
            (
                InputError::UnsupportedUrlScheme,
                spec(MediaKind::Text, DataRef::Url("file:///tmp/a".into())),
            ),
            (
                InputError::TextCannotConvert,
                InputSpec {
                    kind: MediaKind::Text,
                    data: DataRef::Text("hello".into()),
                    convert_to: Some(MediaKind::Audio),
                    convert_timeout_ms: None,
                },
            ),
            (
                InputError::TextCannotConvert,
                InputSpec {
                    kind: MediaKind::Text,
                    data: DataRef::File(absolute("notes.txt")),
                    convert_to: Some(MediaKind::Image),
                    convert_timeout_ms: None,
                },
            ),
            (
                InputError::ConvertToEqualsKind,
                InputSpec {
                    kind: MediaKind::Audio,
                    data: DataRef::File(absolute("voice.wav")),
                    convert_to: Some(MediaKind::Audio),
                    convert_timeout_ms: None,
                },
            ),
            (
                InputError::ConvertTimeoutNotPositive,
                InputSpec {
                    kind: MediaKind::Video,
                    data: DataRef::File(absolute("clip.mp4")),
                    convert_to: Some(MediaKind::Text),
                    convert_timeout_ms: Some(0),
                },
            ),
            (
                InputError::ConvertTimeoutAboveMax,
                InputSpec {
                    kind: MediaKind::Video,
                    data: DataRef::File(absolute("clip.mp4")),
                    convert_to: Some(MediaKind::Text),
                    convert_timeout_ms: Some(MAX_CONVERT_TIMEOUT_MS + 1),
                },
            ),
        ];
        for (expected, spec) in cases {
            assert_eq!(validate(&spec), Err(expected), "{:?}", spec);
        }
    }

    #[test]
    fn data_ref_serde_is_externally_tagged() {
        for (value, expected) in [
            (json!({"text": "hello"}), DataRef::Text("hello".into())),
            (json!({"file": "voice.wav"}), DataRef::File(PathBuf::from("voice.wav"))),
            (json!({"url": "https://example.com/a"}), DataRef::Url("https://example.com/a".into())),
        ] {
            assert_eq!(serde_json::from_value::<DataRef>(value.clone()).unwrap(), expected);
            assert_eq!(serde_json::to_value(&expected).unwrap(), value);
        }
        assert!(serde_json::from_value::<DataRef>(json!("hello")).is_err());
        assert!(serde_json::from_value::<DataRef>(json!({"path": "x"})).is_err());
        assert!(serde_json::from_value::<DataRef>(json!({"text": "a", "url": "b"})).is_err());
    }

    #[test]
    fn input_spec_round_trips_and_refuses_unknown_fields() {
        let spec = InputSpec {
            kind: MediaKind::Audio,
            data: DataRef::File(PathBuf::from("voice.wav")),
            convert_to: Some(MediaKind::Text),
            convert_timeout_ms: Some(900_000),
        };
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            value,
            json!({
                "kind": "audio",
                "data": {"file": "voice.wav"},
                "convert_to": "text",
                "convert_timeout_ms": 900_000
            })
        );
        assert_eq!(serde_json::from_value::<InputSpec>(value).unwrap(), spec);
        let minimal: InputSpec =
            serde_json::from_value(json!({"kind": "text", "data": {"text": "hello"}})).unwrap();
        assert_eq!(minimal.convert_to, None);
        assert_eq!(minimal.convert_timeout_ms, None);
        assert!(serde_json::from_value::<InputSpec>(
            json!({"kind": "audio", "data": {"file": "x"}, "extra": 1})
        )
        .is_err());
    }

    #[test]
    fn display_name_never_returns_inline_text() {
        assert_eq!(display_name(&DataRef::Text("secret".into())), "");
        assert_eq!(
            display_name(&DataRef::File(PathBuf::from("clips/quick-brown-fox.wav"))),
            "quick-brown-fox.wav"
        );
        assert_eq!(display_name(&DataRef::File(PathBuf::from("/"))), "");
        assert_eq!(
            display_name(&DataRef::Url("https://example.com:8443/a/b?q=secret#f".into())),
            "example.com"
        );
        assert_eq!(
            display_name(&DataRef::Url("https://user:pass@host.example/x".into())),
            "host.example"
        );
        assert_eq!(display_name(&DataRef::Url("https://[::1]:8080/x".into())), "::1");
    }
}
