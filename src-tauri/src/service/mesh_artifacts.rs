use std::collections::HashMap;

use crate::api::mesh_attachment::{build_ingest_request, parse_ingest_response, AttachmentKind};
use crate::mesh::{transport::PeerClient, types::RelayCommand};

#[derive(Debug, PartialEq, Eq)]
struct Directive {
    start: usize,
    end: usize,
    tag: String,
    path: String,
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
                    let path = path.trim();
                    let path = if path.starts_with(['\'', '"']) {
                        path.chars()
                            .next()
                            .and_then(|quote| path.strip_prefix(quote)?.strip_suffix(quote))
                    } else {
                        Some(path)
                    };
                    if let Some(path) = path.filter(|path| !path.is_empty()) {
                        result.push(Directive {
                            start: offset,
                            end: offset + content.len(),
                            tag: tag.into(),
                            path: path.into(),
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
    paths: &HashMap<String, String>,
) -> Result<String, String> {
    let mut output = String::new();
    let mut offset = 0;
    for directive in plan {
        let path = paths
            .get(&directive.path)
            .ok_or("Artifact transfer did not return a target path")?;
        if path.is_empty() || path.contains(['"', '\r', '\n']) {
            return Err("Artifact transfer returned an invalid target path".into());
        }
        output.push_str(&text[offset..directive.start]);
        output.push_str(&format!("{} \"{}\"", directive.tag, path));
        offset = directive.end;
    }
    output.push_str(&text[offset..]);
    Ok(output)
}

pub(crate) async fn transfer(
    client: &PeerClient,
    method: &str,
    path: &str,
    body: &mut serde_json::Value,
) -> Result<(), String> {
    transfer_with(method, path, body, |source, ingest_path| async move {
        let source_path = source.clone();
        let request = tokio::task::spawn_blocking(move || {
            let source_path = crate::commands::html_preview::checked_path(&source_path)?;
            build_ingest_request(&source_path, AttachmentKind::File)
        })
        .await
        .map_err(|error| error.to_string())??;
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
    })
    .await
}

async fn transfer_with<F, Fut>(
    method: &str,
    path: &str,
    body: &mut serde_json::Value,
    mut transfer_file: F,
) -> Result<(), String>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
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
    if plans.iter().map(|(_, _, plan)| plan.len()).sum::<usize>() > 32 {
        return Err("A remote message may contain at most 32 file previews".into());
    }
    let mut transferred = HashMap::new();
    for (_, _, plan) in &plans {
        for directive in plan {
            if transferred.contains_key(&directive.path) {
                continue;
            }
            let target = transfer_file(directive.path.clone(), ingest_path.clone()).await?;
            transferred.insert(directive.path.clone(), target);
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
