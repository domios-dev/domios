use std::sync::{Arc, OnceLock};
use std::time::Instant;

use codeterm_types::config::SupporterConfig;
use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    DecisionRequest, TokenMode,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::mem_routes::decision::SupporterRegistry;
use crate::mem::decision::token_store::TokenStore;

mod activity;
mod journal;

pub(crate) use activity::SupporterRuntime;
pub(crate) use journal::{Action, ActionRecord, VerdictKind};

#[derive(Default)]
pub(crate) struct TokenSnapshot {
    pub(crate) usable: bool,
}

impl TokenSnapshot {
    pub(crate) fn read(store: &TokenStore, adapter: &str) -> Self {
        Self {
            usable: store
                .status(adapter)
                .ok()
                .is_some_and(|status| status.present && status.mode_ok != Some(false))
                && store.load(adapter).ok().flatten().is_some(),
        }
    }
}

pub(crate) fn supporter_connected(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
    token_store: &TokenSnapshot,
) -> bool {
    !cfg.adapter.is_empty()
        && registry
            .get(&cfg.adapter)
            .is_some_and(|model| model.token_mode() != TokenMode::Required || token_store.usable)
}

pub(crate) fn runtime() -> Arc<SupporterRuntime> {
    static RUNTIME: OnceLock<Arc<SupporterRuntime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Arc::new(SupporterRuntime::new(
                codeterm_paths::data_dir().join("supporter"),
            ))
        })
        .clone()
}

pub(crate) fn criteria_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn budgeted_model(
    model: Arc<dyn DecisionModel>,
    runtime: Arc<SupporterRuntime>,
    cap: usize,
    kind: VerdictKind,
    tab: Option<String>,
    criteria_version: String,
) -> Arc<dyn DecisionModel> {
    Arc::new(BudgetedModel {
        model,
        runtime,
        cap,
        kind,
        tab,
        criteria_version,
    })
}

struct BudgetedModel {
    model: Arc<dyn DecisionModel>,
    runtime: Arc<SupporterRuntime>,
    cap: usize,
    kind: VerdictKind,
    tab: Option<String>,
    criteria_version: String,
}

impl DecisionModel for BudgetedModel {
    fn id(&self) -> &str {
        self.model.id()
    }
    fn token_mode(&self) -> TokenMode {
        self.model.token_mode()
    }
    fn model_id(&self) -> Option<String> {
        self.model.model_id()
    }
    fn primitives(&self) -> DecisionPrimitives {
        self.model.primitives()
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        self.model.models()
    }
    fn select_model(&self, id: &str) -> bool {
        self.model.select_model(id)
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        self.runtime
            .reserve(self.cap, self.kind, self.tab.as_deref(), 1)?;
        let answer = self.model.decide(request, deadline);
        if let Ok(answer) = &answer {
            let state_bytes = serde_json::to_vec(&request.state).unwrap_or_default();
            self.runtime.verdict(
                self.kind,
                self.tab.as_deref(),
                answer,
                self.model.model_id(),
                &self.criteria_version,
                &criteria_hash(&state_bytes),
            );
        }
        answer
    }
}

pub(crate) fn activity_status(runtime: &SupporterRuntime, cap: usize) -> Value {
    runtime.status(cap)
}

#[cfg(test)]
mod tests;
