use axum::{
    body::{Body, Bytes},
    http::StatusCode,
};
use futures_util::StreamExt;
use std::{
    io::{Read, Write},
    sync::Arc,
};
use tokio::sync::mpsc;

const INLINE_LIMIT: usize = 64 * 1024;
pub(crate) const IO_CHUNK: usize = 32 * 1024;
pub(crate) const ENVELOPE_LIMIT: usize = 32 * 1024 * 1024;
pub(crate) const UPLOAD_LIMIT: usize = crate::web_routes::MAX_TEMP_FILE_DECODED_BYTES + 1024 * 1024;
type Failure = (StatusCode, String);

pub(crate) enum Stored {
    Inline(Bytes),
    File(Arc<tempfile::NamedTempFile>),
}

fn io(error: impl ToString) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

pub(crate) async fn receive(body: Body, limit: usize) -> Result<Stored, Failure> {
    let mut chunks = body.into_data_stream();
    let mut inline = Vec::new();
    let mut writer = None;
    let mut total = 0usize;
    while let Some(chunk) = chunks.next().await {
        let bytes = chunk.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        total = total.checked_add(bytes.len()).ok_or_else(|| {
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Envelope exceeds its body limit".into(),
            )
        })?;
        if total > limit {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "Envelope exceeds its body limit".into(),
            ));
        }
        if writer.is_none() && total <= INLINE_LIMIT {
            inline.extend_from_slice(&bytes);
            continue;
        }
        if writer.is_none() {
            let (tx, mut rx) = mpsc::channel::<Bytes>(2);
            let task = tokio::task::spawn_blocking(
                move || -> Result<tempfile::NamedTempFile, std::io::Error> {
                    let mut file = tempfile::NamedTempFile::new()?;
                    while let Some(bytes) = rx.blocking_recv() {
                        file.write_all(&bytes)?;
                    }
                    file.flush()?;
                    Ok(file)
                },
            );
            let initial = Bytes::from(std::mem::take(&mut inline));
            for start in (0..initial.len()).step_by(IO_CHUNK) {
                tx.send(initial.slice(start..(start + IO_CHUNK).min(initial.len())))
                    .await
                    .map_err(io)?;
            }
            writer = Some((tx, task));
        }
        if let Some((tx, _)) = &writer {
            for start in (0..bytes.len()).step_by(IO_CHUNK) {
                tx.send(bytes.slice(start..(start + IO_CHUNK).min(bytes.len())))
                    .await
                    .map_err(io)?;
            }
        }
    }
    match writer {
        Some((tx, task)) => {
            drop(tx);
            Ok(Stored::File(Arc::new(task.await.map_err(io)?.map_err(io)?)))
        }
        None => Ok(Stored::Inline(Bytes::from(inline))),
    }
}

impl Stored {
    pub async fn relay_policy(&self) -> Option<super::rpc::model::RelayOperation> {
        match self {
            Self::Inline(bytes) => serde_json::from_slice(bytes).ok(),
            Self::File(file) => {
                let file = file.clone();
                tokio::task::spawn_blocking(move || {
                    file.reopen()
                        .ok()
                        .and_then(|reader| serde_json::from_reader(reader).ok())
                })
                .await
                .ok()
                .flatten()
            }
        }
    }

    pub fn into_body(self) -> Body {
        match self {
            Self::Inline(bytes) => Body::from(bytes),
            Self::File(file) => {
                let (tx, mut rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
                let task = tokio::task::spawn_blocking(move || {
                    let mut reader = match file.reopen() {
                        Ok(reader) => reader,
                        Err(error) => {
                            let _ = tx.blocking_send(Err(error));
                            return;
                        }
                    };
                    let mut buffer = vec![0u8; IO_CHUNK];
                    loop {
                        match reader.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(size) => {
                                if tx
                                    .blocking_send(Ok(Bytes::copy_from_slice(&buffer[..size])))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(error) => {
                                let _ = tx.blocking_send(Err(error));
                                break;
                            }
                        }
                    }
                    drop(file);
                });
                Body::from_stream(async_stream::stream! {
                    while let Some(bytes) = rx.recv().await { yield bytes; }
                    let _ = task.await;
                })
            }
        }
    }
}
