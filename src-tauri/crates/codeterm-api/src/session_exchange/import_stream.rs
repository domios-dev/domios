use std::io::{BufReader, Read, Seek, SeekFrom};

use axum::body::Body;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use codeterm_service_core::session::store::SessionSnapshot;
use codeterm_types::plan::Plan;

#[derive(Deserialize)]
struct Fixture {
    format: String,
    sanitized: bool,
    snapshot: SessionSnapshot,
    plans: Vec<Plan>,
}

pub(super) fn decode(reader: impl Read) -> Result<(SessionSnapshot, Vec<Plan>), String> {
    let fixture: Fixture = serde_json::from_reader(reader).map_err(|error| error.to_string())?;
    if !fixture.sanitized || fixture.format != super::FORMAT {
        return Err(format!(
            "Import requires an {} sanitized document",
            super::FORMAT
        ));
    }
    codeterm_service_core::service::session_persist::validate_fixture_snapshot(&fixture.snapshot)?;
    Ok((fixture.snapshot, fixture.plans))
}

pub(super) async fn receive(body: Body) -> Result<(SessionSnapshot, Vec<Plan>), String> {
    tokio::time::timeout(std::time::Duration::from_secs(90), spool(body))
        .await
        .map_err(|_| "Fixture upload deadline elapsed".to_string())?
}

async fn spool(body: Body) -> Result<(SessionSnapshot, Vec<Plan>), String> {
    let mut file = tokio::task::spawn_blocking(tempfile::tempfile)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let writer = file.try_clone().map_err(|error| error.to_string())?;
    let mut writer = tokio::fs::File::from_std(writer);
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        for bytes in chunk.chunks(64 * 1024) {
            writer
                .write_all(bytes)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    writer.flush().await.map_err(|error| error.to_string())?;
    drop(writer);
    tokio::task::spawn_blocking(move || {
        file.seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        decode(BufReader::new(file))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests;
