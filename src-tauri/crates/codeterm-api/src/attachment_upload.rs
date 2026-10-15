use axum::{
    body::{Body, Bytes},
    extract::{FromRequest, Multipart, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use std::{io::Write, path::PathBuf};
use tokio::sync::mpsc;

use crate::{
    routes::{check_auth, AppState},
    web_routes::{sanitize_filename, MAX_TEMP_FILE_DECODED_BYTES},
};
use codeterm_util::attachments::{bucket_dir, evict, EvictionPolicy, CHAT_UPLOAD_CACHE_BYTES};

pub(crate) struct UploadError {
    status: StatusCode,
    kind: &'static str,
    message: String,
}

impl UploadError {
    fn new(status: StatusCode, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            kind,
            message: message.into(),
        }
    }

    fn io(error: impl ToString) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_unavailable",
            error.to_string(),
        )
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({
            "error": { "kind": self.kind, "message": self.message, "limit_bytes": MAX_TEMP_FILE_DECODED_BYTES }
        }))).into_response()
    }
}

enum UploadPart {
    Bytes(Bytes),
    Commit,
}

pub(crate) async fn streaming_multipart(request: Request) -> Result<Multipart, UploadError> {
    let request = request.map(|body| {
        let chunks = body.into_data_stream().then(|chunk| async move {
            // Multer drains ready frames into one buffer; yield so it parses each frame before reading more.
            tokio::task::yield_now().await;
            chunk
        });
        Body::from_stream(chunks)
    });
    Multipart::from_request(request, &())
        .await
        .map_err(|error| {
            UploadError::new(
                StatusCode::BAD_REQUEST,
                "invalid_multipart",
                error.to_string(),
            )
        })
}

fn write_upload(
    dir: PathBuf,
    name: String,
    mut chunks: mpsc::Receiver<UploadPart>,
) -> Result<PathBuf, UploadError> {
    let staging = dir.join(".partial");
    std::fs::create_dir_all(&staging).map_err(UploadError::io)?;
    let mut file = tempfile::NamedTempFile::new_in(staging).map_err(UploadError::io)?;
    while let Some(part) = chunks.blocking_recv() {
        match part {
            UploadPart::Bytes(bytes) => file.write_all(&bytes).map_err(UploadError::io)?,
            UploadPart::Commit => {
                file.flush().map_err(UploadError::io)?;
                let path = dir.join(format!("{}-{name}", uuid::Uuid::new_v4().simple()));
                file.persist(&path).map_err(UploadError::io)?;
                evict(&dir, EvictionPolicy::MaxBytes(CHAT_UPLOAD_CACHE_BYTES));
                return Ok(path);
            }
        }
    }
    Err(UploadError::new(
        StatusCode::BAD_REQUEST,
        "upload_interrupted",
        "Upload interrupted",
    ))
}

pub(crate) async fn receive_upload(
    mut multipart: Multipart,
    dir: PathBuf,
    limit: usize,
) -> Result<Json<serde_json::Value>, UploadError> {
    let mut field = multipart
        .next_field()
        .await
        .map_err(|error| {
            UploadError::new(
                StatusCode::BAD_REQUEST,
                "invalid_multipart",
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            UploadError::new(
                StatusCode::BAD_REQUEST,
                "missing_file",
                "Missing file field",
            )
        })?;
    if field.name() != Some("file") {
        return Err(UploadError::new(
            StatusCode::BAD_REQUEST,
            "missing_file",
            "Expected file field",
        ));
    }
    let name = sanitize_filename(field.file_name().unwrap_or("upload.bin"));
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    let (sender, receiver) = mpsc::channel(2);
    let writer = tokio::task::spawn_blocking(move || write_upload(dir, name, receiver));
    let received = async {
        let mut size = 0usize;
        while let Some(chunk) = field.chunk().await.map_err(|error| {
            UploadError::new(
                StatusCode::BAD_REQUEST,
                "upload_interrupted",
                error.to_string(),
            )
        })? {
            size = size.checked_add(chunk.len()).ok_or_else(|| {
                UploadError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "attachment_too_large",
                    "Attachment exceeds the upload size limit",
                )
            })?;
            if size > limit {
                return Err(UploadError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "attachment_too_large",
                    "Attachment exceeds the upload size limit",
                ));
            }
            sender
                .send(UploadPart::Bytes(chunk))
                .await
                .map_err(UploadError::io)?;
        }
        if size == 0 {
            return Err(UploadError::new(
                StatusCode::BAD_REQUEST,
                "empty_file",
                "Empty attachment",
            ));
        }
        sender
            .send(UploadPart::Commit)
            .await
            .map_err(UploadError::io)?;
        Ok(size)
    }
    .await;
    drop(sender);
    let stored = writer.await.map_err(UploadError::io)?;
    let size = received?;
    let path = stored?;
    Ok(Json(
        serde_json::json!({ "path": path, "size": size, "content_type": content_type }),
    ))
}

pub(crate) async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<serde_json::Value>, UploadError> {
    check_auth(&state, &headers)
        .map_err(|(status, error)| UploadError::new(status, "unauthorized", error))?;
    receive_upload(
        streaming_multipart(request).await?,
        bucket_dir("chat"),
        MAX_TEMP_FILE_DECODED_BYTES,
    )
    .await
}

#[cfg(test)]
mod tests;
