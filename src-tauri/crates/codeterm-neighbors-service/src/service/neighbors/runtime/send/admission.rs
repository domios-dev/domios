use super::*;
use std::collections::BTreeMap;

pub(super) async fn reserve(
    service: &ServiceContext,
    metadata: &Metadata,
    message_id: &str,
    generation: u64,
) -> codeterm_neighbors::Result<()> {
    let _guard = service
        .extensions
        .get_or_init(super::super::super::history::HistoryCell::default)
        .0
        .lock()
        .await;
    let path = format!("send-admissions/{}/{message_id}.json", &message_id[..2]);
    if metadata.read::<bool>(path.clone()).await?.is_some() {
        return Err(Error::OperationIdConflict);
    }
    let index: BTreeMap<String, String> = metadata
        .read(format!("history-index-{}.json", &message_id[..2]))
        .await?
        .unwrap_or_default();
    if index.contains_key(message_id) {
        return Err(Error::OperationIdConflict);
    }
    service.require_neighbor_history_generation(generation)?;
    // Reservations live with account metadata, never expiring with bodies, failures, or sign-out.
    metadata.write(path, true).await
}
