use axum::{
    body::{Body, Bytes},
    extract::{FromRequest, Multipart, Request},
    http::{header, HeaderMap, StatusCode},
};
use serde_json::{Map, Value};

use super::{WebhookRejection, MAX_WEBHOOK_BODY_BYTES};

const MAX_TEXT_FIELD_BYTES: usize = 64 * 1024;
const MAX_FIELDS: usize = 16;
const MAX_FIELD_NAME_BYTES: usize = 128;

pub(crate) struct AudioPart {
    pub file_name: String,
    pub media_type: String,
    pub bytes: Bytes,
}

pub(super) struct Decoded {
    pub fields: Value,
    pub audio: Option<AudioPart>,
}

fn invalid_multipart() -> WebhookRejection {
    WebhookRejection::new(
        StatusCode::BAD_REQUEST,
        "invalid_multipart",
        "Malformed multipart form; let the sender set Content-Type and its boundary",
    )
}

fn invalid_fields() -> WebhookRejection {
    WebhookRejection::new(
        StatusCode::BAD_REQUEST,
        "invalid_form_fields",
        "Multipart fields must have unique names and contain valid UTF-8 text",
    )
}

fn duplicate_audio() -> WebhookRejection {
    WebhookRejection::new(
        StatusCode::BAD_REQUEST,
        "invalid_form_fields",
        "A webhook form may carry at most one audio part",
    )
}

pub(super) fn body_limit(audio_cap: Option<usize>) -> usize {
    MAX_WEBHOOK_BODY_BYTES + audio_cap.unwrap_or(0)
}

fn audio_file_name(declared: Option<&str>, media_type: &str) -> String {
    if let Some(name) = declared.filter(|name| !name.trim().is_empty()) {
        return crate::api::web_routes::sanitize_filename(name);
    }
    let extension = match media_type {
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" | "audio/aac" => "m4a",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/ogg" | "audio/opus" => "ogg",
        "audio/webm" => "webm",
        "audio/flac" | "audio/x-flac" => "flac",
        _ => "bin",
    };
    format!("audio.{extension}")
}

/// `audio_cap` is `Some` only for a receiver that declares audio; every other receiver skips file parts.
pub(super) async fn decode(
    headers: &HeaderMap,
    body: &Bytes,
    audio_cap: Option<usize>,
) -> Result<Decoded, WebhookRejection> {
    if body.len() > body_limit(audio_cap) {
        return Err(WebhookRejection::body_too_large());
    }
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().to_ascii_lowercase())
        .ok_or_else(WebhookRejection::unsupported_media)?;
    if media_type == "application/json"
        || (media_type.starts_with("application/") && media_type.ends_with("+json"))
    {
        if body.len() > MAX_WEBHOOK_BODY_BYTES {
            return Err(WebhookRejection::body_too_large());
        }
        let fields = serde_json::from_slice(body).map_err(|_| WebhookRejection::invalid_json())?;
        return Ok(Decoded {
            fields,
            audio: None,
        });
    }
    if media_type != "multipart/form-data" {
        return Err(WebhookRejection::unsupported_media());
    }

    let mut request = Request::new(Body::from(body.clone()));
    *request.headers_mut() = headers.clone();
    // Without an explicit limit the multipart extractor silently applies its own 2 MiB default.
    axum::extract::DefaultBodyLimit::max(body_limit(audio_cap)).apply(&mut request);
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|_| invalid_multipart())?;
    let mut fields = Map::new();
    let mut seen_names = std::collections::HashSet::new();
    let mut audio: Option<AudioPart> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| invalid_multipart())?
    {
        let name = field.name().ok_or_else(invalid_fields)?.to_owned();
        if name.is_empty() || name.len() > MAX_FIELD_NAME_BYTES || !seen_names.insert(name.clone())
        {
            return Err(invalid_fields());
        }
        if seen_names.len() > MAX_FIELDS {
            return Err(WebhookRejection::body_too_large());
        }
        let part_media = field.content_type().map(str::to_ascii_lowercase);
        let is_file = field.file_name().is_some()
            || part_media
                .as_deref()
                .is_some_and(|media| !media.starts_with("text/"));
        let declared_name = field.file_name().map(str::to_owned);
        let bytes = field.bytes().await.map_err(|_| invalid_multipart())?;
        if is_file {
            let Some(cap) = audio_cap else { continue };
            let Some(part_media) = part_media.filter(|media| media.starts_with("audio/")) else {
                continue;
            };
            if audio.is_some() {
                return Err(duplicate_audio());
            }
            if bytes.len() > cap {
                return Err(WebhookRejection::body_too_large());
            }
            if bytes.is_empty() {
                continue;
            }
            audio = Some(AudioPart {
                file_name: audio_file_name(declared_name.as_deref(), &part_media),
                media_type: part_media,
                bytes,
            });
            continue;
        }
        if bytes.len() > MAX_TEXT_FIELD_BYTES {
            return Err(WebhookRejection::body_too_large());
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid_fields())?;
        fields.insert(name, Value::String(text.to_owned()));
    }
    let audio_len = audio.as_ref().map_or(0, |audio| audio.bytes.len());
    if body.len() - audio_len > MAX_WEBHOOK_BODY_BYTES {
        return Err(WebhookRejection::body_too_large());
    }
    Ok(Decoded {
        fields: Value::Object(fields),
        audio,
    })
}

#[cfg(test)]
mod tests;
