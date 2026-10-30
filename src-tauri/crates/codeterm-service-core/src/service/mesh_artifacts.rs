use std::collections::HashMap;

use crate::mesh::{transport::PeerClient, types::RelayCommand};
use crate::service::mesh_attachment::{
    parse_ingest_response, AttachmentKind, IngestRequest, MESH_ATTACHMENT_MAX_BYTES,
};

#[derive(Debug, PartialEq, Eq)]
enum ArtifactError {
    Policy(&'static str),
    Transfer(String),
}

impl From<String> for ArtifactError {
    fn from(error: String) -> Self {
        Self::Transfer(error)
    }
}

fn preview_error(error: String) -> ArtifactError {
    let reason = codeterm_local_preview::html_preview::refusal_reason(&error);
    match reason {
        "private_path" | "unreferenced" | "size_limit" | "invalid_path" | "unsafe_file" => {
            ArtifactError::Policy(reason)
        }
        _ => ArtifactError::Transfer(error),
    }
}

use codeterm_local_preview::artifact_directive::{
    scan_artifacts as scan_author_text, serialize_artifact, FileCard,
};

#[cfg(test)]
fn directives(text: &str) -> Vec<FileCard> {
    scan_author_text(text, true).cards
}

fn ingest_path(method: &str, path: &str) -> Option<String> {
    if !method.eq_ignore_ascii_case("POST") {
        return None;
    }
    let bare = path.split('?').next()?;
    let rest = bare.strip_prefix("/api/tabs/")?;
    let (tab, action) = rest.split_once('/')?;
    if tab.is_empty() || !matches!(action, "send" | "chat-send" | "delayed-messages") {
        return None;
    }
    Some(format!("/api/tabs/{tab}/attachments"))
}

fn rewrite(
    text: &str,
    plan: &[FileCard],
    paths: &HashMap<String, String>,
) -> Result<String, String> {
    let mut output = String::new();
    let mut offset = 0;
    for card in plan {
        let targets = card
            .paths
            .iter()
            .map(|source| {
                paths
                    .get(source)
                    .cloned()
                    .ok_or("Artifact transfer did not return a target path")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let replacement = serialize_artifact(&card.kind, &targets)
            .ok_or("Artifact transfer returned an invalid target path")?;
        output.push_str(&text[offset..card.start]);
        output.push_str(&replacement);
        offset = card.end;
    }
    output.push_str(&text[offset..]);
    Ok(output)
}

pub async fn transfer(
    svc: &super::ServiceContext,
    source_tab: Option<&str>,
    client: &PeerClient,
    method: &str,
    path: &str,
    body: &mut serde_json::Value,
) -> Result<(), String> {
    let source_tab = source_tab.map(str::to_string);
    transfer_with(method, path, body, |source, ingest_path| {
        let svc = svc.clone();
        let source_tab = source_tab.clone();
        async move {
            let source_path = source.clone();
            let request = tokio::task::spawn_blocking(move || {
                let configured = crate::config::load_config().api.preview_secret_paths;
                let file = codeterm_local_preview::html_preview::checked_path(&source_path)
                    .map_err(preview_error)?;
                let references = [codeterm_local_preview::references::path_key(&file)].into();
                let policy = super::local_preview::artifact_policy(
                    &svc,
                    source_tab.as_deref(),
                    references,
                    &configured,
                )
                .map_err(ArtifactError::Transfer)?;
                artifact_request(&source_path, &policy)
            })
            .await
            .map_err(|error| ArtifactError::Transfer(error.to_string()))??;
            let command = RelayCommand {
                method: "POST".into(),
                path: ingest_path,
                body: serde_json::to_value(request).map_err(|error| error.to_string())?,
                delivery_id: None,
            };
            let response = client
                .send_relay(&command)
                .await
                .map_err(|error| error.to_string())?;
            Ok(parse_ingest_response(&source, response)?.path)
        }
    })
    .await
}

fn artifact_request(
    source: &str,
    policy: &codeterm_local_preview::PreviewPolicy,
) -> Result<IngestRequest, ArtifactError> {
    use base64::Engine as _;
    if source != source.trim() {
        return Err(ArtifactError::Policy("invalid_path"));
    }
    let bytes = codeterm_local_preview::html_preview::read_artifact_file(
        source,
        policy,
        MESH_ATTACHMENT_MAX_BYTES,
    )
    .map_err(preview_error)?;
    if bytes.is_empty() {
        return Err(ArtifactError::Policy("empty_file"));
    }
    Ok(IngestRequest {
        kind: AttachmentKind::File,
        name: std::path::Path::new(source)
            .file_name()
            .ok_or(ArtifactError::Policy("invalid_path"))?
            .to_string_lossy()
            .into_owned(),
        size: bytes.len() as u64,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

async fn transfer_with<F, Fut>(
    method: &str,
    path: &str,
    body: &mut serde_json::Value,
    mut transfer_file: F,
) -> Result<(), String>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<String, ArtifactError>>,
{
    let Some(ingest_path) = ingest_path(method, path) else {
        return Ok(());
    };
    let mut pointers = vec!["/text".to_string()];
    if let Some(batch) = body.get("batch").and_then(|value| value.as_array()) {
        pointers.extend((0..batch.len()).map(|index| format!("/batch/{index}/text")));
    }
    let plans: Vec<_> = pointers
        .into_iter()
        .filter_map(|pointer| {
            let text = body.pointer(&pointer)?.as_str()?.to_string();
            let scan = scan_author_text(&text, true);
            (!scan.cards.is_empty() || scan.overflow).then_some((pointer, text, scan))
        })
        .collect();
    if plans.iter().any(|(_, _, scan)| scan.overflow)
        || plans
            .iter()
            .flat_map(|(_, _, scan)| &scan.cards)
            .map(|card| card.paths.len())
            .sum::<usize>()
            > 32
    {
        return Err("A remote message may contain at most 32 file previews".into());
    }
    let mut transferred = HashMap::new();
    for (_, _, scan) in &plans {
        for source in scan.cards.iter().flat_map(|card| &card.paths) {
            if transferred.contains_key(source) {
                continue;
            }
            let target = match transfer_file(source.clone(), ingest_path.clone()).await {
                Ok(path) => path,
                Err(ArtifactError::Policy(reason)) => {
                    return Err(format!("Artifact refused: {reason}"))
                }
                Err(ArtifactError::Transfer(error)) => return Err(error),
            };
            transferred.insert(source.clone(), target);
        }
    }
    let replacements = plans
        .into_iter()
        .map(|(pointer, text, scan)| {
            rewrite(&text, &scan.cards, &transferred).map(|text| (pointer, text))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (pointer, text) in replacements {
        let field = body
            .pointer_mut(&pointer)
            .ok_or("Remote message text changed during transfer")?;
        *field = serde_json::Value::String(text);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
