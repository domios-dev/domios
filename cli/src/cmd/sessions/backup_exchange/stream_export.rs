use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

struct StagedExport(PathBuf);

impl Drop for StagedExport {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Deserialize)]
struct ExportHeader {
    format: String,
    sanitized: bool,
}

fn validate(reader: impl Read) -> Result<(), String> {
    let header: ExportHeader = serde_json::from_reader(reader)
        .map_err(|error| format!("Invalid snapshot response: {error}"))?;
    if !header.sanitized || header.format != super::FORMAT {
        return Err("Snapshot export returned an unsupported or unsanitized document".into());
    }
    Ok(())
}

fn receive(mut reader: impl Read, file: &mut File) -> Result<(), String> {
    std::io::copy(&mut reader, file).map_err(|error| format!("Read snapshot: {error}"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("Read staged snapshot: {error}"))?;
    validate(BufReader::new(file))
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(90))
        .default_headers(crate::client::identity_headers(&[
            ("CODETERM_TAB_ID", "x-codeterm-tab-id"),
            ("CODETERM_SESSION_ID", "x-codeterm-session-id"),
            ("CODETERM_SESSION_BINDING_NONCE", "x-codeterm-binding-nonce"),
            ("CODETERM_WORKER_ID", "x-codeterm-worker-id"),
        ]))
        .build()
        .map_err(|error| error.to_string())
}

pub(super) fn download(
    base: &str,
    token: Option<&str>,
    name: &str,
    out: &Path,
) -> Result<(), String> {
    let client = client()?;
    // Export bodies stream to disk instead of entering the bounded api.request RPC collector.
    let mut request = client.get(format!(
        "{base}/api/session/backups/{}/export",
        urlencoding::encode(name)
    ));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let mut body = Vec::new();
        response
            .take(4097)
            .read_to_end(&mut body)
            .map_err(|error| error.to_string())?;
        return super::decode_response(status, &body).map(|_| ());
    }
    let staging_path = out.with_file_name(format!(".domios-export-{}.json", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&staging_path)
        .map_err(|error| format!("Stage snapshot: {error}"))?;
    let staged = StagedExport(staging_path);
    let received = receive(response, &mut file);
    drop(file);
    received?;
    std::fs::copy(&staged.0, out).map_err(|error| format!("Write snapshot: {error}"))?;
    Ok(())
}

pub(super) fn upload(
    base: &str,
    token: Option<&str>,
    path: &Path,
) -> Result<serde_json::Value, String> {
    let mut file = crate::local_admission::open_local_file(path)
        .map_err(|error| format!("Read fixture: {error}"))?;
    validate(BufReader::new(&mut file))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("Read fixture: {error}"))?;
    let mut request = client()?
        .post(format!("{base}/api/session/backups/import"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(file);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|error| error.to_string())?;
    let status = response.status();
    let mut body = Vec::new();
    response
        .take(4097)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    super::decode_response(status, &body)
}

#[cfg(test)]
mod tests;
