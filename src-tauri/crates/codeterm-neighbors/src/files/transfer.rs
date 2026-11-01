use super::{Binding, Manifest};
use crate::Result;
use async_trait::async_trait;
use zeroize::Zeroizing;

#[async_trait]
pub trait ReceivePorts: Send + Sync {
    async fn download(&self, manifest: &Manifest) -> Result<Vec<u8>>;
    async fn verify(
        &self,
        manifest: &Manifest,
        binding: &Binding<'_>,
        object: Vec<u8>,
    ) -> Result<Zeroizing<Vec<u8>>>;
    async fn save(&self, manifest: &Manifest, plaintext: Zeroizing<Vec<u8>>) -> Result<()>;
    async fn confirm(&self, manifest: &Manifest, outcome: &'static str) -> Result<()>;
}

pub async fn receive(
    ports: &dyn ReceivePorts,
    manifest: &Manifest,
    binding: &Binding<'_>,
) -> Result<()> {
    manifest.validate(binding)?;
    let object = ports.download(manifest).await?;
    let plaintext = match ports.verify(manifest, binding, object).await {
        Ok(plaintext) => plaintext,
        Err(error) => {
            ports.confirm(manifest, "rejected").await?;
            return Err(error);
        }
    };
    ports.save(manifest, plaintext).await?;
    ports.confirm(manifest, "saved").await
}

#[cfg(test)]
mod tests;
