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

pub(super) async fn decode(headers: &HeaderMap, body: &Bytes) -> Result<Value, WebhookRejection> {
    if body.len() > MAX_WEBHOOK_BODY_BYTES {
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
        return serde_json::from_slice(body).map_err(|_| WebhookRejection::invalid_json());
    }
    if media_type != "multipart/form-data" {
        return Err(WebhookRejection::unsupported_media());
    }

    let mut request = Request::new(Body::from(body.clone()));
    *request.headers_mut() = headers.clone();
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|_| invalid_multipart())?;
    let mut fields = Map::new();
    let mut seen_names = std::collections::HashSet::new();
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
        let is_file = field.file_name().is_some()
            || field
                .content_type()
                .is_some_and(|media| !media.to_ascii_lowercase().starts_with("text/"));
        let bytes = field.bytes().await.map_err(|_| invalid_multipart())?;
        if is_file {
            continue;
        }
        if bytes.len() > MAX_TEXT_FIELD_BYTES {
            return Err(WebhookRejection::body_too_large());
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid_fields())?;
        fields.insert(name, Value::String(text.to_owned()));
    }
    Ok(Value::Object(fields))
}

#[cfg(test)]
mod tests;
