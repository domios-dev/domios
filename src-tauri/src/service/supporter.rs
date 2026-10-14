use std::sync::{Arc, OnceLock};
use std::time::Instant;

use codeterm_decision::{
    deadline, DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPricing,
    DecisionPrimitives, DecisionQuestionsRequest, DecisionRequest, DecisionResponse,
    NamedDecisionQuestion, TokenMode,
};
use codeterm_types::config::SupporterConfig;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::mem_routes::decision::SupporterRegistry;
use crate::mem::decision::token_store::TokenStore;

mod activity;
mod backoff;
pub(crate) mod connections;
mod feed;
mod journal;
mod narrative;
mod physical;
pub(crate) mod rerank;
mod spend;
mod storage;

pub(crate) use activity::SupporterRuntime;
pub(crate) use journal::{Action, ActionRecord, VerdictKind};
pub(crate) use narrative::sentinel as sentinel_questions;
pub(crate) use narrative::{
    provider_update as update_questions, recovery as recovery_questions, tab_state as tab_questions,
};
pub(crate) use narrative::{ActivityContext, QuestionAnswer};

#[derive(Default)]
pub(crate) struct TokenSnapshot {
    pub(crate) usable: bool,
    pub(crate) acl_invalid: bool,
}

impl TokenSnapshot {
    pub(crate) fn read(store: &TokenStore, adapter: &str) -> Self {
        let status = store.status(adapter).ok();
        let present = status.as_ref().is_some_and(|status| status.present);
        let mode_ok = status.as_ref().and_then(|status| status.mode_ok);
        let token_readable =
            present && mode_ok != Some(false) && store.load(adapter).ok().flatten().is_some();
        Self::from_status(present, mode_ok, token_readable)
    }

    pub(crate) fn from_status(present: bool, mode_ok: Option<bool>, token_readable: bool) -> Self {
        Self {
            usable: present && mode_ok != Some(false) && token_readable,
            acl_invalid: present && mode_ok == Some(false),
        }
    }
}

pub(crate) fn read_token_snapshot(
    runtime: &SupporterRuntime,
    store: &TokenStore,
    adapter: &str,
) -> TokenSnapshot {
    let snapshot = TokenSnapshot::read(store, adapter);
    report_token_acl_state(runtime, adapter, &snapshot);
    snapshot
}

pub(crate) fn report_token_acl_state(
    runtime: &SupporterRuntime,
    adapter: &str,
    snapshot: &TokenSnapshot,
) {
    runtime.token_acl_state_changed(adapter, snapshot.acl_invalid);
}

pub(crate) fn supporter_connected(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
    token_store: &TokenSnapshot,
) -> bool {
    configured_model(cfg, registry)
        .is_some_and(|model| model.token_mode() != TokenMode::Required || token_store.usable)
}

pub(crate) fn configured_model(
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

pub(crate) fn restore_model(
    cfg: &SupporterConfig,
    id: &str,
    adapter: &Arc<dyn DecisionModel>,
) -> bool {
    let model = cfg
        .connection(id)
        .model
        .or_else(|| adapter.models().first().map(|model| model.id.clone()));
    model.as_deref().is_none_or(|model| {
        adapter.select_model(model) && adapter.model_id().as_deref() == Some(model)
    })
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

pub(crate) fn runtime_with_notifications(svc: &super::ServiceContext) -> Arc<SupporterRuntime> {
    let runtime = runtime();
    let mux = svc.mux.clone();
    let app = svc.app_handle.clone();
    let title_mux = svc.mux.clone();
    runtime.attach_tab_titles(Arc::new(move |tab| {
        title_mux.read().get_tab(tab).map(|tab| tab.title.clone())
    }));
    runtime.attach_visibility(
        Arc::new(move |tab| mux.read().get_tab(tab).and_then(|tab| tab.team_id.clone())),
        Arc::new(move |event| {
            crate::api::events::emit_everywhere(&app, "supporter-journal-appended", event)
        }),
    );
    let svc = svc.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| {
        svc.log_system_notice(
            alert.title,
            None,
            Some(
                if alert.level == codeterm_types::notification::NotifyLevel::Failure {
                    "error"
                } else {
                    "warning"
                },
            ),
        );
    }));
    runtime
}

pub(crate) fn criteria_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn budgeted_model_with_activity(
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

pub(crate) fn activity_status(runtime: &SupporterRuntime, cap: usize) -> Value {
    let mut status = runtime.status(cap);
    status["observation_scheduler"] = super::supporter_tab_state::scheduler::status();
    status
}

#[cfg(test)]
mod requests_tests;
#[cfg(test)]
mod tests;
