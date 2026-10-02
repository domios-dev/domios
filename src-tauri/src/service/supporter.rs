use std::sync::{Arc, OnceLock};
use std::time::Instant;

use codeterm_types::config::SupporterConfig;
use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPricing,
    DecisionPrimitives, DecisionQuestionsRequest, DecisionRequest, DecisionResponse,
    NamedDecisionQuestion, TokenMode,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::mem_routes::decision::SupporterRegistry;
use crate::mem::decision::token_store::TokenStore;

mod activity;
pub(crate) mod connections;
mod feed;
mod journal;
mod spend;
mod storage;

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
    runtime.attach_visibility(
        Arc::new(move |tab| mux.read().get_tab(tab).and_then(|tab| tab.team_id.clone())),
        Arc::new(move |event| {
            crate::api::events::emit_everywhere(&app, "supporter-journal-appended", event)
        }),
    );
    let svc = svc.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| {
        svc.create_notification_ext(
            alert.title,
            None,
            Some(
                if alert.level == codeterm_types::notification::NotifyLevel::Failure {
                    "error"
                } else {
                    "warning"
                },
            ),
            None,
            None,
            super::notification::NotificationOrigin::default(),
            super::notification::PushIntent {
                level: Some(alert.level),
                event_key: Some(alert.event_key),
            },
            None,
        );
    }));
    runtime
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
    criteria_version: Option<String>,
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
    criteria_version: Option<String>,
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
    fn pricing(&self) -> Option<DecisionPricing> {
        self.model.pricing()
    }
    fn metadata(&self) -> codeterm_types::decision::DecisionMetadata {
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
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        if request.questions.is_empty() {
            return Err(DecisionError::Parse("empty_questions".into()));
        }
        self.runtime
            .reserve_request(
                self.cap,
                self.kind,
                self.tab.as_deref(),
                request.questions.len(),
                self.model.id(),
            )
            .inspect_err(|error| self.runtime.failure(self.kind, self.tab.as_deref(), error))?;
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
        self.runtime
            .record_spend(self.model.id(), price.as_ref(), &response);
        let response = response
            .inspect_err(|error| self.runtime.failure(self.kind, self.tab.as_deref(), error))?;
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        request.validate_answers(&response.answers)?;
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
                model_id,
                criteria_version,
                state_hash: &criteria_hash(&serde_json::to_vec(&request.state).unwrap_or_default()),
                question_set_hash: &question_set_hash,
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
    runtime.status(cap)
}

#[cfg(test)]
mod requests_tests;
#[cfg(test)]
mod tests;
