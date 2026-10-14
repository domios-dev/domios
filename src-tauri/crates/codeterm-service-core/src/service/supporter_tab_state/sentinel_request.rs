use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::{identity, safe_response, ServiceContext, ShadowHost};
use crate::agent::tab_state::TabStateSnapshot;
use crate::multiplexer::types::Tab;
use crate::sentinel::machine::SafeKind;

const REQUEST_TABS: usize = 256;
const GLOBAL_REQUEST_LIMIT: usize = 600;
const GLOBAL_REQUEST_WINDOW: Duration = Duration::from_secs(60);

static REQUESTS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static GLOBAL_REQUESTS: LazyLock<Mutex<VecDeque<Instant>>> = LazyLock::new(Default::default);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestEffect {
    Held(Option<u64>),
    AlreadyClaimed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RequestRefusal {
    UnsupportedKind,
    PromptChanged,
    KindMismatch,
    UnsafeTarget,
    ObservationUnavailable,
    RequestLimit,
}

impl RequestRefusal {
    pub(crate) fn reason(&self) -> &'static str {
        match self {
            Self::UnsupportedKind => "sentinel_prompt_kind_unsupported",
            Self::PromptChanged => "sentinel_prompt_identity_changed",
            Self::KindMismatch => "sentinel_prompt_kind_mismatch",
            Self::UnsafeTarget => "sentinel_prompt_target_refused",
            Self::ObservationUnavailable => "sentinel_prompt_observation_unavailable",
            Self::RequestLimit => "sentinel_prompt_request_limit",
        }
    }
}

fn safe_kind(kind: &SafeKind) -> Option<safe_response::Kind> {
    match kind {
        SafeKind::DirectoryTrust => Some(safe_response::Kind::DirectoryTrust),
        SafeKind::Onboarding => Some(safe_response::Kind::Onboarding),
        SafeKind::Update => Some(safe_response::Kind::Update),
        SafeKind::Permission => Some(safe_response::Kind::Permission),
        SafeKind::Structured | SafeKind::Other(_) => None,
    }
}

fn prompt_value(tab: &Tab) -> Option<Value> {
    let prompt = tab.pending_prompt.as_ref()?;
    Some(json!({
        "kind": tab.pending_prompt_kind,
        "question": prompt.question,
        "options": prompt.options,
        "multi_select": prompt.multi_select,
    }))
}

fn claim(tab: &str, identity: &str) -> Result<bool, RequestRefusal> {
    let mut requests = REQUESTS.lock();
    if requests.get(tab).is_some_and(|prior| prior == identity) {
        return Ok(true);
    }
    if !requests.contains_key(tab) && requests.len() >= REQUEST_TABS {
        return Err(RequestRefusal::RequestLimit);
    }
    let now = Instant::now();
    let mut global = GLOBAL_REQUESTS.lock();
    while global
        .front()
        .is_some_and(|at| now.saturating_duration_since(*at) >= GLOBAL_REQUEST_WINDOW)
    {
        global.pop_front();
    }
    if global.len() >= GLOBAL_REQUEST_LIMIT {
        return Err(RequestRefusal::RequestLimit);
    }
    global.push_back(now);
    requests.insert(tab.into(), identity.into());
    Ok(false)
}

impl ServiceContext {
    pub(crate) fn request_safe_prompt_response(
        &self,
        tab_id: &str,
        kind: &SafeKind,
        expected_prompt: &Value,
        requested_by: &str,
    ) -> Result<RequestEffect, RequestRefusal> {
        let kind = safe_kind(kind).ok_or(RequestRefusal::UnsupportedKind)?;
        let expected_prompt_kind: codeterm_types::pty::PromptKind =
            serde_json::from_value(expected_prompt["kind"].clone())
                .map_err(|_| RequestRefusal::KindMismatch)?;
        let (tab, local) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or(RequestRefusal::PromptChanged)?;
            let current_prompt = prompt_value(tab).ok_or(RequestRefusal::PromptChanged)?;
            if current_prompt != *expected_prompt {
                return Err(RequestRefusal::PromptChanged);
            }
            if tab.pending_prompt_kind != Some(expected_prompt_kind) {
                return Err(RequestRefusal::KindMismatch);
            }
            if !crate::service::unattended_launch::owns_prompt_answers(tab) {
                return Err(RequestRefusal::UnsafeTarget);
            }
            (tab.clone(), identity::LocalIdentity::of_tab(tab))
        };
        let pty_id = tab.pty_id.as_deref().ok_or(RequestRefusal::PromptChanged)?;
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(pty_id)
            .ok_or(RequestRefusal::PromptChanged)?;
        let target = self
            .sentinel_safe_response_target(tab_id, &screen, kind)
            .ok_or(RequestRefusal::UnsafeTarget)?;
        if target.prompt != *tab.pending_prompt.as_ref().unwrap()
            || target.kind.prompt_kind() != expected_prompt_kind
        {
            return Err(RequestRefusal::KindMismatch);
        }
        let live = self.mux.read().get_tab(tab_id).is_some_and(|current| {
            identity::LocalIdentity::of_tab(current) == local
                && prompt_value(current).as_ref() == Some(expected_prompt)
        });
        if !live {
            return Err(RequestRefusal::PromptChanged);
        }
        let prompt_occurrence = self.ledger_snapshot(tab_id).prompt;
        let request_identity = crate::service::supporter::criteria_hash(
            &serde_json::to_vec(&json!([
                local.launch_key,
                expected_prompt,
                prompt_occurrence,
                kind.context()
            ]))
            .unwrap_or_default(),
        );
        match claim(tab_id, &request_identity)? {
            true => return Ok(RequestEffect::AlreadyClaimed),
            false => {}
        }
        let snapshot = TabStateSnapshot {
            tab_id: tab.id.clone(),
            agent_type: tab
                .agent_type
                .clone()
                .ok_or(RequestRefusal::PromptChanged)?,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            screen_text: screen,
        };
        if self.scrape_model_connected() {
            self.observe_sentinel_prompt_candidate(snapshot, &local, kind, requested_by, None)
                .map(|observation_id| RequestEffect::Held(Some(observation_id)))
                .ok_or(RequestRefusal::ObservationUnavailable)
        } else {
            self.resolve_scrape_candidate_fallback(&snapshot);
            Ok(RequestEffect::Held(None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_respond_prompt_occurrence_claims_once_and_replaces_changed_prompt() {
        let tab = format!("tab-{}", uuid::Uuid::new_v4().simple());
        assert!(!claim(&tab, "prompt-a").unwrap());
        assert!(claim(&tab, "prompt-a").unwrap());
        assert!(!claim(&tab, "prompt-b").unwrap());
        assert!(claim(&tab, "prompt-b").unwrap());
    }

    #[test]
    fn sentinel_respond_prompt_global_rate_window_is_bounded() {
        assert_eq!(GLOBAL_REQUEST_LIMIT, 600);
        assert_eq!(GLOBAL_REQUEST_WINDOW, Duration::from_secs(60));
    }

    #[test]
    fn sentinel_respond_prompt_supported_kinds_match_safe_response_targets() {
        assert_eq!(
            safe_kind(&SafeKind::Onboarding),
            Some(safe_response::Kind::Onboarding)
        );
        assert_eq!(
            safe_kind(&SafeKind::Permission),
            Some(safe_response::Kind::Permission)
        );
        assert_eq!(
            safe_kind(&SafeKind::Update),
            Some(safe_response::Kind::Update)
        );
        assert_eq!(
            safe_kind(&SafeKind::DirectoryTrust),
            Some(safe_response::Kind::DirectoryTrust)
        );
        assert_eq!(safe_kind(&SafeKind::Structured), None);
    }
}
