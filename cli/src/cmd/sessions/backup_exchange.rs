use std::path::Path;

use crate::{cmd::print, local_api::LocalApiClient};
use serde_json::Value;

fn response(request: crate::local_api::LocalApiRequest) -> Value {
    let response = request
        .send()
        .unwrap_or_else(|error| print::fail(1, &error.to_string()));
    let status = response.status();
    let value: Value = response
        .json()
        .unwrap_or_else(|error| print::fail(1, &format!("Invalid snapshot response: {error}")));
    if !status.is_success() {
        print::fail(1, &format!("Snapshot exchange refused: {status} {value}"));
    }
    value
}

pub(super) fn export(
    json: bool,
    http: &LocalApiClient,
    base: &str,
    token: Option<&str>,
    name: &str,
    sanitize: bool,
    out: &Path,
) {
    let resolved = if out.exists() {
        out.canonicalize()
    } else {
        let parent = out
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        parent
            .canonicalize()
            .map(|parent| parent.join(out.file_name().unwrap_or_default()))
    }
    .unwrap_or_else(|error| print::fail(1, &format!("Resolve export destination: {error}")));
    if resolved.components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        part == ".codeterm" || part.starts_with(".codeterm-dev")
    }) {
        print::fail(
            1,
            "Export destination must be outside every instance data directory",
        );
    }
    if !sanitize {
        print::fail(1, "Snapshot export requires --sanitize");
    }
    let mut request = http.get(format!(
        "{base}/api/session/backups/{}/export",
        urlencoding::encode(name)
    ));
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let value = response(request);
    if value["sanitized"] != true || value["format"] != "upgrade-restore-v1" {
        print::fail(
            1,
            "Snapshot export returned an unsupported or unsanitized document",
        );
    }
    let bytes = serde_json::to_vec_pretty(&value)
        .unwrap_or_else(|error| print::fail(1, &error.to_string()));
    std::fs::write(out, bytes)
        .unwrap_or_else(|error| print::fail(1, &format!("Write snapshot: {error}")));
    if json {
        println!("{}", serde_json::json!({"out":out,"sanitized":true}));
    } else {
        println!("Sanitized read-only desk export: {}", out.display());
    }
}

pub(super) fn import(
    _json: bool,
    http: &LocalApiClient,
    base: &str,
    token: Option<&str>,
    file: &Path,
) {
    let bytes = std::fs::read(file)
        .unwrap_or_else(|error| print::fail(1, &format!("Read fixture: {error}")));
    let value: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| print::fail(1, &format!("Invalid fixture: {error}")));
    if value["sanitized"] != true || value["format"] != "upgrade-restore-v1" {
        print::fail(
            1,
            "Import requires an upgrade-restore-v1 sanitized document",
        );
    }
    let mut request = http
        .post(format!("{base}/api/session/backups/import"))
        .json(&value);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let result = response(request);
    println!("{result}");
}
