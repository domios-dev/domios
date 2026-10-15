use std::sync::{Arc, OnceLock};
use std::time::Instant;

use codeterm_decision::{
    deadline, DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPricing,
    DecisionPrimitives, DecisionQuestionsRequest, DecisionRequest, DecisionResponse,
    NamedDecisionQuestion, TokenMode,
};
use codeterm_types::config::SupporterConfig;
use sha2::{Digest, Sha256};

use codeterm_mem::decision::token_store::TokenStore;

mod activity;
mod backoff;
pub mod connections;
mod feed;
mod journal;
mod narrative;
mod physical;
pub type SupporterRegistry = codeterm_plugin::capability::CapabilityRegistry<dyn DecisionModel>;
pub mod rerank;
mod spend;
mod storage;

pub use activity::SupporterRuntime;
pub use journal::Action;
pub use journal::ActionRecord;
pub use journal::VerdictKind;
pub use narrative::sentinel as sentinel_questions;
pub use narrative::ActivityContext;
pub use narrative::QuestionAnswer;
pub use narrative::{
    provider_update as update_questions, recovery as recovery_questions, tab_state as tab_questions,
};

#[derive(Default)]
pub struct TokenSnapshot {
    pub usable: bool,
    pub acl_invalid: bool,
}

impl TokenSnapshot {
    pub fn read(store: &TokenStore, adapter: &str) -> Self {
        let status = store.status(adapter).ok();
        let present = status.as_ref().is_some_and(|status| status.present);
        let mode_ok = status.as_ref().and_then(|status| status.mode_ok);
        let token_readable =
            present && mode_ok != Some(false) && store.load(adapter).ok().flatten().is_some();
        Self::from_status(present, mode_ok, token_readable)
    }

    pub fn from_status(present: bool, mode_ok: Option<bool>, token_readable: bool) -> Self {
        Self {
            usable: present && mode_ok != Some(false) && token_readable,
            acl_invalid: present && mode_ok == Some(false),
        }
    }
}

pub fn read_token_snapshot(
    runtime: &SupporterRuntime,
    store: &TokenStore,
    adapter: &str,
) -> TokenSnapshot {
    let snapshot = TokenSnapshot::read(store, adapter);
    report_token_acl_state(runtime, adapter, &snapshot);
    snapshot
}

pub fn report_token_acl_state(runtime: &SupporterRuntime, adapter: &str, snapshot: &TokenSnapshot) {
    runtime.token_acl_state_changed(adapter, snapshot.acl_invalid);
}

pub fn supporter_connected(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
    token_store: &TokenSnapshot,
) -> bool {
    configured_model(cfg, registry)
        .is_some_and(|model| model.token_mode() != TokenMode::Required || token_store.usable)
}

pub fn configured_model(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
) -> Option<Arc<dyn DecisionModel>> {
    let id = cfg.active_id()?;
    let adapter = registry.get(id)?;
    if cfg
        .connection(id)
        .model
        .or_else(|| adapter.models().first().map(|model| model.id.clone()))
        .as_deref()
        .is_some_and(|model| adapter.model_id().as_deref() != Some(model))
    {
        return None;
    }
    Some(adapter)
}

pub fn restore_model(cfg: &SupporterConfig, id: &str, adapter: &Arc<dyn DecisionModel>) -> bool {
    let model = cfg
        .connection(id)
        .model
        .or_else(|| adapter.models().first().map(|model| model.id.clone()));
    model.as_deref().is_none_or(|model| {
        adapter.select_model(model) && adapter.model_id().as_deref() == Some(model)
    })
}

pub fn runtime() -> Arc<SupporterRuntime> {
    static RUNTIME: OnceLock<Arc<SupporterRuntime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Arc::new(SupporterRuntime::new(
                codeterm_paths::data_dir().join("supporter"),
            ))
        })
        .clone()
}

pub fn criteria_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn budgeted_model_with_activity(
    model: Arc<dyn DecisionModel>,
    runtime: Arc<SupporterRuntime>,
    cap: usize,
    kind: VerdictKind,
    tab: Option<String>,
    criteria_version: Option<String>,
    activity_context: Option<ActivityContext>,
) -> Arc<dyn DecisionModel> {
    Arc::new(BudgetedModel {
        model,
        runtime,
        cap,
        kind,
        tab,
        criteria_version,
        physical_limit: matches!(kind, VerdictKind::TabState | VerdictKind::Rerank).then_some(1),
        activity_context,
    })
}

struct BudgetedModel {
    model: Arc<dyn DecisionModel>,
    runtime: Arc<SupporterRuntime>,
    cap: usize,
    kind: VerdictKind,
    tab: Option<String>,
    criteria_version: Option<String>,
    physical_limit: Option<usize>,
    activity_context: Option<ActivityContext>,
}

impl DecisionModel for BudgetedModel {
    fn supports_calibrated_choice_confidence(&self) -> bool {
        self.model.supports_calibrated_choice_confidence()
    }
    fn supports_question_batch(&self) -> bool {
        self.model.supports_question_batch()
    }
    fn id(&self) -> &str {
        self.model.id()
    }
    fn token_mode(&self) -> TokenMode {
        self.model.token_mode()
    }
    fn model_id(&self) -> Option<String> {
        self.model.model_id()
    }
    fn pricing(&self) -> Option<DecisionPricing> {
        self.model.pricing()
    }
    fn metadata(&self) -> codeterm_decision::DecisionMetadata {
        self.model.metadata()
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

    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        deadline: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        if deadline::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        if request.questions.is_empty() {
            return Err(DecisionError::Parse("empty_questions".into()));
        }
        self.runtime
            .admit_adapter(self.model.id())
            .inspect_err(|error| {
                self.runtime.failure_with_activity(
                    Some(self.model.id()),
                    self.kind,
                    self.tab.as_deref(),
                    error,
                    self.activity_context.as_ref(),
                )
            })?;
        self.runtime
            .admit_request_budget(self.cap)
            .inspect_err(|error| {
                self.runtime.failure_with_activity(
                    Some(self.model.id()),
                    self.kind,
                    self.tab.as_deref(),
                    error,
                    self.activity_context.as_ref(),
                )
            })?;
        let meter = physical::Meter::new(
            self.runtime.clone(),
            self.cap,
            self.kind,
            self.tab.clone(),
            self.model.id().into(),
            request.questions.len(),
            deadline,
            self.physical_limit,
        );
        let _active = physical::ActiveMeter(meter.clone());
        let _scope = codeterm_decision::physical::RequestScope::enter(meter.clone());
        let price = self.model.pricing();
        let model_id = self.model.model_id();
        let response = self
            .model
            .decide_questions(request, deadline)
            .map_err(|error| {
                if error == DecisionError::Http(402) {
                    DecisionError::CreditExhausted
                } else {
                    error
                }
            });
        let response = match meter.finish() {
            Some(error) => Err(error),
            None => response,
        };
        self.runtime
            .record_spend(self.model.id(), price.as_ref(), &response);
        let response = response.inspect_err(|error| {
            self.runtime.failure_with_activity(
                Some(self.model.id()),
                self.kind,
                self.tab.as_deref(),
                error,
                self.activity_context.as_ref(),
            )
        })?;
        if deadline::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        request
            .validate_answers(&response.answers)
            .map_err(|_| DecisionError::Parse("invalid_response".into()))?;
        self.runtime.mark_adapter_ready(self.model.id());
        let question_set_hash =
            criteria_hash(&serde_json::to_vec(&request.questions).unwrap_or_default());
        let criteria_version = self
            .criteria_version
            .as_deref()
            .unwrap_or(&question_set_hash);
        self.runtime.response(
            self.kind,
            self.tab.as_deref(),
            &response,
            activity::DecisionEvidence {
                decision_point: self
                    .activity_context
                    .as_ref()
                    .map(ActivityContext::decision_point),
                model_id,
                criteria_version,
                state_hash: &criteria_hash(&serde_json::to_vec(&request.state).unwrap_or_default()),
                question_set_hash: &question_set_hash,
                asked: self
                    .activity_context
                    .as_ref()
                    .map(|context| context.answers(&response.answers)),
            },
        );
        Ok(response)
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        let request = DecisionQuestionsRequest {
            state: request.state.clone(),
            questions: std::collections::BTreeMap::from([(
                "q".into(),
                NamedDecisionQuestion {
                    instructions: request.instructions.clone(),
                    question: request.question.clone(),
                },
            )]),
        };
        self.decide_questions(&request, deadline)?
            .answers
            .remove("q")
            .ok_or_else(|| DecisionError::Parse("missing_answer".into()))
    }
}

#[cfg(test)]
mod requests_tests;
#[cfg(test)]
mod tests;

pub mod diagnostics;
pub mod policy;
pub mod privacy;
pub mod process_tree;
