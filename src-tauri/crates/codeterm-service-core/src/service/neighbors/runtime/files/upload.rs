use super::*;
use async_trait::async_trait;

#[async_trait]
trait Put: Send + Sync {
    fn current(&self) -> codeterm_neighbors::Result<()> { Ok(()) }
    async fn put(&self, url: &str, headers: &serde_json::Map<String, Value>, object: Vec<u8>) -> codeterm_neighbors::Result<()>;
}

struct HttpPut<'a> { service: &'a ServiceContext, generation: u64 }

#[async_trait]
impl Put for HttpPut<'_> {
    fn current(&self) -> codeterm_neighbors::Result<()> {
        self.service.require_neighbor_history_generation(self.generation)
    }
    async fn put(&self, url: &str, headers: &serde_json::Map<String, Value>, object: Vec<u8>) -> codeterm_neighbors::Result<()> {
        self.current()?;
        let url = reqwest::Url::parse(url).map_err(|_| Error::Transport)?;
        if url.scheme() != "https" { return Err(Error::Transport); }
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(60)).build().map_err(|_| Error::Transport)?;
        let mut request = client.put(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str().ok_or(Error::Transport)?);
        }
        let response = request.body(object).send().await.map_err(|_| Error::Transport)?;
        if !response.status().is_success() && response.status() != reqwest::StatusCode::PRECONDITION_FAILED { return Err(Error::Transport); }
        self.current()
    }
}

pub(super) async fn upload(service: &ServiceContext, transport: &FirebaseTransport, envelope: &Envelope, manifest: &Manifest, object: Vec<u8>) -> codeterm_neighbors::Result<()> {
    upload_with(transport, envelope, manifest, object, &HttpPut { service, generation: service.neighbor_history_generation() }).await
}

async fn upload_with(transport: &FirebaseTransport, envelope: &Envelope, manifest: &Manifest, object: Vec<u8>, put: &dyn Put) -> codeterm_neighbors::Result<()> {
    let (md5, object) = tokio::task::spawn_blocking(move || (STANDARD.encode(Md5::digest(&object)), object)).await.map_err(|_| Error::Gateway)?;
    put.current()?;
    let grant = transport.calls.invoke("friendsFileGrant", json!({
        "installation_id":transport.installation_id,"friendship_id":envelope.friendship_id,
        "message_id":envelope.message_id,"file_id":manifest.file_id,"ciphertext_size":manifest.ciphertext_size,
        "ciphertext_sha256":manifest.ciphertext_sha256,"ciphertext_md5_b64":md5,
    })).await?;
    if grant["schema_version"] != 1 || grant["file_id"] != manifest.file_id || grant["object_id"] != manifest.object_id { return Err(Error::Transport); }
    if grant["state"] == "granted" {
        let upload = &grant["upload"];
        if upload["method"] != "PUT" { return Err(Error::Transport); }
        let url = upload["url"].as_str().ok_or(Error::Transport)?;
        put.current()?;
        put.put(url, upload["headers"].as_object().ok_or(Error::Transport)?, object).await?;
    } else if grant["state"] != "ready" { return Err(Error::Transport); }
    put.current()?;
    let finalized = transport.calls.invoke("friendsFileFinalize", json!({"installation_id":transport.installation_id,"file_id":manifest.file_id})).await?;
    if finalized["schema_version"] != 1 || finalized["file_id"] != manifest.file_id || finalized["object_id"] != manifest.object_id
        || finalized["state"] != "ready" || finalized["ciphertext_size"] != manifest.ciphertext_size || finalized["ciphertext_sha256"] != manifest.ciphertext_sha256 {
        return Err(Error::Transport);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
