use super::*;
use crate::service::neighbors::history::HistoryCell;
use codeterm_neighbors::registry::ReadSignal;

impl ServiceContext {
    pub(super) async fn apply_neighbor_read_signal(
        &self,
        metadata: &Metadata,
        uid: &str,
        signal: &ReadSignal,
        generation: u64,
    ) -> Result<bool> {
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbors_enabled()?;
        self.require_neighbor_history_generation(generation)?;
        let mut history = metadata.history(&signal.friendship_id).await?;
        let known = history.entries.iter().any(|entry| {
            entry.direction == "out"
                && entry.sender_uid == uid
                && entry.message_id == signal.message_id
        });
        if !history.apply_read_through(uid, &signal.message_id, signal.read_at) {
            return Ok(known);
        }
        self.require_neighbor_history_generation(generation)?;
        metadata
            .write(format!("history-{}.json", signal.friendship_id), history)
            .await?;
        self.ui.emit(
            "neighbor-history-changed",
            json!({"friendship_id":signal.friendship_id}),
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
