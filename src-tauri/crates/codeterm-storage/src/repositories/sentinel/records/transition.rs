use super::*;

impl TransitionRecord {
    pub(in crate::repositories::sentinel) fn validate(&self) -> Result<(), StorageError> {
        optional_text("causal run", &self.causal_run_json)?;
        optional_text("decision provenance", &self.decision_json)?;
        identifier("transition id", &self.id)?;
        identifier("transition bucket", &self.bucket)?;
        integer("transition time", self.accepted_at)?;
        count("transition results", self.handlers.len(), MAX_RESULTS)?;
        let mut handlers = HashSet::new();
        for result in &self.handlers {
            result.validate()?;
            if !handlers.insert(result.handler_id.as_str()) {
                return Err(StorageError::invalid(
                    "transition results",
                    "repeat a handler id",
                ));
            }
        }
        Ok(())
    }

    pub(in crate::repositories::sentinel) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.id.len()
            + self.bucket.len()
            + optional_len(&self.decision_json)
            + optional_len(&self.causal_run_json)
            + self
                .handlers
                .iter()
                .map(HandlerResultRecord::cost)
                .sum::<usize>()
    }
}

impl NewTransition {
    pub(in crate::repositories::sentinel) fn validate(&self) -> Result<(), StorageError> {
        optional_text("causal run", &self.causal_run_json)?;
        optional_text("decision provenance", &self.decision_json)?;
        identifier("transition id", &self.id)?;
        identifier("transition bucket", &self.bucket)?;
        integer("transition time", self.accepted_at)
    }

    pub(in crate::repositories::sentinel) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.id.len()
            + self.bucket.len()
            + optional_len(&self.decision_json)
            + optional_len(&self.causal_run_json)
    }
}
