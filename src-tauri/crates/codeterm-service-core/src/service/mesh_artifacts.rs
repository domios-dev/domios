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

#[derive(Debug, PartialEq, Eq)]
struct Directive {
    start: usize,
    end: usize,
    tag: String,
    paths: Vec<String>,
}

fn directives(text: &str) -> Vec<Directive> {
    let mut result = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(char, usize)> = None;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let trimmed = content.trim_start();
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        if let Some(marker) = marker {
            let width = trimmed.chars().take_while(|c| *c == marker).count();
            if width >= 3 {
                if fence.is_none() {
                    fence = Some((marker, width));
                } else if fence.is_some_and(|(open, count)| open == marker && width >= count) {
                    fence = None;
                }
            }
        }
        if fence.is_none() && !content.starts_with("    ") && !content.starts_with('\t') {
            if let Some((tag, path)) = trimmed.split_once(char::is_whitespace) {
                if [
                    "@ds-image",
                    "@ds-page",
                    "@ds-document",
                    "@ct-image",
                    "@ct-page",
                    "@ct-document",
                ]
                .iter()
                .any(|known| tag.eq_ignore_ascii_case(known))
                {
                    let paths =
                        codeterm_local_preview::artifact_directive::artifact_paths(tag, path);
                    if !paths.is_empty() {
                        result.push(Directive {
                            start: offset,
                            end: offset + content.len(),
                            tag: tag.into(),
                            paths: paths.into_iter().map(str::to_owned).collect(),
                        });
                    }
                }
            }
        }
        offset += line.len();
    }
    result
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
    plan: &[Directive],
    paths: &HashMap<String, Result<String, &'static str>>,
) -> Result<String, String> {
    let mut output = String::new();
    let mut offset = 0;
    for directive in plan {
        let mut targets = Vec::new();
        let mut refused = Vec::new();
        for source in &directive.paths {
            let path = paths
                .get(source)
                .ok_or("Artifact transfer did not return a target path")?;
            match path {
                Ok(path) => {
                    if path.is_empty() || path.contains(['"', '\r', '\n']) {
                        return Err("Artifact transfer returned an invalid target path".into());
                    }
                    targets.push(format!("\"{path}\""));
                }
                Err(reason) => refused.push(format!("[Artifact refused: {reason}]")),
            }
        }
        output.push_str(&text[offset..directive.start]);
        if !targets.is_empty() {
            output.push_str(&format!("{} {}", directive.tag, targets.join(" ")));
            if !refused.is_empty() {
                output.push(' ');
            }
        }
        output.push_str(&refused.join(" "));
        offset = directive.end;
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
            let plan = directives(&text);
            (!plan.is_empty()).then_some((pointer, text, plan))
        })
        .collect();
    if plans
        .iter()
        .flat_map(|(_, _, plan)| plan)
        .map(|directive| directive.paths.len())
        .sum::<usize>()
        > 32
    {
        return Err("A remote message may contain at most 32 file previews".into());
    }
    let mut transferred = HashMap::new();
    for (_, _, plan) in &plans {
        for source in plan.iter().flat_map(|directive| &directive.paths) {
            if transferred.contains_key(source) {
                continue;
            }
            let target = match transfer_file(source.clone(), ingest_path.clone()).await {
                Ok(path) => Ok(path),
                Err(ArtifactError::Policy(reason)) => Err(reason),
                Err(ArtifactError::Transfer(error)) => return Err(error),
            };
            transferred.insert(source.clone(), target);
        }
    }
    for (pointer, text, plan) in plans {
        let text = rewrite(&text, &plan, &transferred)?;
        let field = body
            .pointer_mut(&pointer)
            .ok_or("Remote message text changed during transfer")?;
        *field = serde_json::Value::String(text);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
