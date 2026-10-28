use super::*;
use codeterm_blueprint_types::blueprint_document::calls::{
    self, CallPin, FrozenClosure, MemoryResolver,
};
pub(crate) fn freeze(
    resolver: Option<&dyn MemoryResolver>,
    document: Document,
) -> Result<Document, String> {
    let has_calls = document.nodes().any(|node| node.call.is_some());
    if !has_calls && document.dependencies.is_empty() {
        return Ok(document);
    }
    let resolver = resolver.ok_or("Call pins require a caller Team/workspace")?;
    let closure = calls::resolve_saved(document, resolver).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let context = calls::check_context(&closure, &Default::default());
    let diagnostics = codeterm_blueprint_types::blueprint_document::checker::check_ir(
        &closure.root.ir(),
        &context,
    );
    if codeterm_blueprint_types::blueprint_document::checker::has_errors(&diagnostics) {
        return Err(
            codeterm_blueprint_types::blueprint_document::checker::render(&diagnostics, true),
        );
    }
    Ok(closure.document())
}
impl MemManager {
    pub fn save_blueprint_scoped(
        &self,
        area: &str,
        team: &str,
        document: Document,
        intents: &[String],
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        let mut prepared = self.qualify_blueprint(area, document, intents, evidence)?;
        prepared.team = Some(team.into());
        let saved = self.persist_blueprint(area, prepared, true, AREA_CAP)?;
        self.index_after_save(&saved.id);
        Ok(saved)
    }
    pub fn enqueue_blueprint_scoped(
        self: &std::sync::Arc<Self>,
        area: String,
        team: String,
        definition: serde_json::Value,
        intents: Vec<String>,
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        self.guard_commit()?;
        let mut prepared = self.qualify_blueprint(
            &area,
            Document::from_definition(&definition)?,
            &intents,
            evidence,
        )?;
        prepared.team = Some(team);
        let (reply, completion) = std::sync::mpsc::sync_channel(1);
        self.queue_blueprint(Pending::Keep {
            area,
            prepared,
            reply,
        })?;
        completion
            .recv()
            .map_err(|_| "Blueprint memory worker stopped before save completion")?
    }
    pub fn pin_blueprint_calls(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Document, String> {
        self.guard_commit()?;
        self.store
            .blueprint_pin_calls(&Self::blueprint_write_context(), area, team, document)
    }
    pub fn pin_blueprint_bundle(
        &self,
        area: &str,
        team: &str,
        document: Document,
        local_callees: &[calls::LocalCallee],
    ) -> Result<Document, String> {
        self.guard_commit()?;
        self.store.blueprint_pin_bundle(
            &Self::blueprint_write_context(),
            area,
            team,
            document,
            local_callees,
        )
    }
    pub fn blueprint_call_drift(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Vec<calls::Drift>, String> {
        self.store.blueprint_call_drift(area, team, document)
    }
    pub fn blueprint_callable(&self, area: &str, team: &str, id: &str) -> Result<Document, String> {
        self.store.blueprint_call_head(area, team, id)
    }
    pub fn blueprint_call_revision(
        &self,
        area: &str,
        team: &str,
        pin: &CallPin,
    ) -> Result<Document, String> {
        self.store.blueprint_call_revision(area, team, pin)
    }
}
#[cfg(test)]
mod tests;

pub(crate) fn freeze_bundle(
    document: Document,
    local: &[calls::LocalCallee],
    resolver: &dyn MemoryResolver,
) -> Result<Document, String> {
    let closure = calls::resolve_bundle(document, local, resolver).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| format!("{}:{}:{}: {}", e.file, e.line, e.column, e.message))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    Ok(closure.document())
}

pub(crate) fn drift(
    closure: &FrozenClosure,
    resolver: &dyn MemoryResolver,
) -> Result<Vec<calls::Drift>, String> {
    closure.drift(resolver)
}
