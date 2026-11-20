use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(PartialEq, Eq)]
pub(super) struct PayloadIdentity {
    pub version: String,
    pub signature: String,
}

pub(super) struct StagedPayload {
    entry: Mutex<Option<(PayloadIdentity, Arc<Vec<u8>>)>>,
}

impl StagedPayload {
    pub const fn new() -> Self {
        Self {
            entry: Mutex::new(None),
        }
    }

    pub async fn get_or_download(
        &self,
        identity: PayloadIdentity,
        download_verified: impl Future<Output = Result<Vec<u8>, String>>,
    ) -> Result<Arc<Vec<u8>>, String> {
        {
            let mut entry = self.entry.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((_, bytes)) = entry.as_ref().filter(|(saved, _)| *saved == identity) {
                return Ok(Arc::clone(bytes));
            }
            *entry = None;
        }
        let bytes = Arc::new(download_verified.await?);
        *self.entry.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((identity, Arc::clone(&bytes)));
        Ok(bytes)
    }

    pub fn clear(&self) {
        *self.entry.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

#[cfg(test)]
mod tests;
