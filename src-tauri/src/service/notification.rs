//! Notification creation with OS toast support.

use super::ServiceContext;
use crate::notification::{
    store::{AgentInputAdmission, NotificationLevel},
    NotifState,
};
use codeterm_types::pty::{AgentLiveState, PendingPrompt, QuestionSubtype, TimelineState};

#[derive(Debug)]
enum NotificationAdmission {
    Diagnostic,
    AgentInputRequest {
        session_id: String,
        prompt_identity: String,
    },
}

fn admit_agent_input_request(
    tab_session_id: Option<&str>,
    session_id: &str,
    state: Option<&AgentLiveState>,
    pending_prompt: Option<&PendingPrompt>,
    requested_identity: Option<&str>,
) -> Result<String, String> {
    if tab_session_id != Some(session_id) {
        return Err("Tab session is no longer authoritative".to_string());
    }
    let state = state.ok_or_else(|| "Tab is not waiting for input".to_string())?;
    // Fail-closed allow-set: Unknown/unclassified verdicts are diagnostic, not
    // notification authority (structurally-minted prompts classify there).
    let actionable_subtype = matches!(
        state.subtype,
        Some(
            QuestionSubtype::ModelChoice
                | QuestionSubtype::ModelPicker
                | QuestionSubtype::Permission
                | QuestionSubtype::Auth
                | QuestionSubtype::Update
        )
    );
    if state.state != TimelineState::Question || !actionable_subtype {
        return Err("Tab is not waiting for actionable user input".to_string());
    }
    // agent_state.question is diagnostic only; admission requires the
    // authoritative pending_prompt record for this tab/session.
    let prompt =
        pending_prompt.ok_or_else(|| "Authoritative pending prompt is not available".to_string())?;
    if prompt.options.is_empty() && prompt.submit_available != Some(true) {
        return Err("Prompt exposes no actuatable input control".to_string());
    }
    let body = Some(prompt.question.trim())
        .filter(|question| !question.is_empty())
        .ok_or_else(|| "Authoritative pending prompt is not available".to_string())?
        .to_string();
    let requested = requested_identity
        .map(str::trim)
        .filter(|identity| !identity.is_empty());
    if requested.is_some_and(|identity| identity != body) {
        return Err("Prompt is no longer authoritative".to_string());
    }
    Ok(body)
}

fn notification_level(level: Option<&str>) -> NotificationLevel {
    match level {
        Some("warning") | Some("Warning") => NotificationLevel::Warning,
        Some("error") | Some("Error") => NotificationLevel::Error,
        _ => NotificationLevel::Info,
    }
}

fn create_notification_with_delivery(
    admission: NotificationAdmission,
    store: &NotifState,
    deliver_native: impl FnOnce(&str, Option<&str>),
    title: String,
    body: Option<String>,
    level: Option<&str>,
    actions: Option<serde_json::Value>,
    sticky: Option<bool>,
    tab_id: Option<String>,
    workspace_id: Option<String>,
    emit: impl FnOnce(serde_json::Value),
) -> String {
    if matches!(admission, NotificationAdmission::Diagnostic) {
        return uuid::Uuid::new_v4().to_string();
    }
    let parsed_level = notification_level(level);
    let level_str = match &parsed_level {
        NotificationLevel::Warning => "Warning",
        NotificationLevel::Error => "Error",
        _ => "Info",
    };
    let id = match admission {
        NotificationAdmission::Diagnostic => unreachable!(),
        NotificationAdmission::AgentInputRequest {
            session_id,
            prompt_identity,
        } => {
            let mut store = store.lock();
            match store.add_agent_input_request(
                tab_id.clone().expect("agent input request has a tab"),
                session_id,
                prompt_identity,
                title.clone(),
                body.clone().expect("agent input request has a body"),
                workspace_id
                    .clone()
                    .expect("agent input request has a workspace"),
            ) {
                AgentInputAdmission::Added(id) => id,
                AgentInputAdmission::Duplicate(id) => return id,
            }
        }
    };

    let mut payload = serde_json::json!({
        "id": id,
        "title": title,
        "body": body,
        "level": level_str,
        "tab_id": tab_id,
        "workspace_id": workspace_id,
    });
    if let Some(ref acts) = actions {
        payload["actions"] = acts.clone();
    }
    if let Some(s) = sticky {
        payload["sticky"] = serde_json::json!(s);
    }
    deliver_native(
        payload["title"].as_str().unwrap_or_default(),
        payload["body"].as_str(),
    );
    emit(payload);
    id
}

impl ServiceContext {
    pub fn create_notification(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
    ) -> String {
        self.create_notification_ext(title, body, level, None, None, None, None)
    }

    /// Extended notification with optional action buttons and sticky flag.
    pub fn create_notification_ext(
        &self,
        title: String,
        body: Option<String>,
        level: Option<&str>,
        actions: Option<serde_json::Value>,
        sticky: Option<bool>,
        tab_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        create_notification_with_delivery(
            NotificationAdmission::Diagnostic,
            &self.notif,
            |title, body| {
                crate::api::os_notify::_send_os_notification(&self.app_handle, title, body)
            },
            title,
            body,
            level,
            actions,
            sticky,
            tab_id,
            workspace_id,
            |payload| self.notify_event("notification-created", payload),
        )
    }

    pub fn create_agent_input_request_notification(
        &self,
        tab_id: String,
        session_id: String,
        prompt_identity: Option<String>,
    ) -> Result<String, String> {
        let (title, body, workspace_id) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(&tab_id)
                .ok_or_else(|| "Tab not found".to_string())?;
            let body = admit_agent_input_request(
                tab.agent_session_id.as_deref(),
                &session_id,
                tab.agent_state.as_ref(),
                tab.pending_prompt.as_ref(),
                prompt_identity.as_deref(),
            )?;
            let workspace_id = mux
                .find_workspace_for_tab(&tab_id)
                .ok_or_else(|| "Owning workspace not found".to_string())?;
            (
                format!("Input needed in Tab {}", tab.title),
                body,
                workspace_id,
            )
        };
        let prompt_identity = body.clone();

        Ok(create_notification_with_delivery(
            NotificationAdmission::AgentInputRequest {
                session_id,
                prompt_identity,
            },
            &self.notif,
            |title, body| {
                crate::api::os_notify::_send_os_notification(&self.app_handle, title, body)
            },
            title,
            Some(body),
            Some("warning"),
            None,
            None,
            Some(tab_id),
            Some(workspace_id),
            |payload| self.notify_event("notification-created", payload),
        ))
    }

    pub fn resolve_agent_input_request_notification(
        &self,
        tab_id: &str,
        session_id: &str,
        prompt_identity: &str,
    ) {
        self.notif
            .lock()
            .resolve_agent_input_request(tab_id, session_id, prompt_identity.trim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::store::NotificationStore;
    use codeterm_types::pty::PromptOption;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn question_state(question: Option<&str>) -> AgentLiveState {
        // Live classifier verdict for "Which deployment region…" prompts.
        AgentLiveState {
            state: TimelineState::Question,
            subtype: Some(QuestionSubtype::ModelChoice),
            status_line: None,
            phase: None,
            elapsed_ms: None,
            last_turn_summary: None,
            question: question.map(str::to_string),
        }
    }

    fn prompt(question: &str) -> PendingPrompt {
        // Genuine interactive prompt: actuatable options.
        PendingPrompt {
            question: question.into(),
            options: vec![
                PromptOption {
                    label: "Yes".into(),
                    value: "1".into(),
                },
                PromptOption {
                    label: "No".into(),
                    value: "2".into(),
                },
            ],
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    // Structurally-minted text: a question with no actuatable input control.
    fn text_prompt(question: &str) -> PendingPrompt {
        PendingPrompt {
            question: question.into(),
            options: Vec::new(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    struct DeliveryProbe {
        store: NotifState,
        native: Mutex<Vec<(String, Option<String>)>>,
        emitted: Mutex<Vec<serde_json::Value>>,
    }

    impl DeliveryProbe {
        fn new() -> Self {
            Self {
                store: Arc::new(Mutex::new(NotificationStore::new(10))),
                native: Mutex::new(Vec::new()),
                emitted: Mutex::new(Vec::new()),
            }
        }

        /// Mirrors `create_agent_input_request_notification`: admission gates
        /// every delivery channel; an Err must reach none of them.
        fn admit_and_deliver(
            &self,
            tab_session_id: Option<&str>,
            session_id: &str,
            state: Option<&AgentLiveState>,
            pending_prompt: Option<&PendingPrompt>,
            requested_identity: Option<&str>,
        ) -> Result<String, String> {
            let body = admit_agent_input_request(
                tab_session_id,
                session_id,
                state,
                pending_prompt,
                requested_identity,
            )?;
            Ok(create_notification_with_delivery(
                NotificationAdmission::AgentInputRequest {
                    session_id: session_id.into(),
                    prompt_identity: body.clone(),
                },
                &self.store,
                |title, native_body| {
                    self.native
                        .lock()
                        .push((title.to_string(), native_body.map(str::to_string)))
                },
                "Input needed in Tab Review".into(),
                Some(body),
                Some("warning"),
                None,
                None,
                Some("tab-owned".into()),
                Some("workspace-owned".into()),
                |payload| self.emitted.lock().push(payload),
            ))
        }

        fn assert_silent(&self) {
            assert_eq!(self.store.lock().list(false).len(), 0);
            assert_eq!(self.native.lock().len(), 0);
            assert_eq!(self.emitted.lock().len(), 0);
        }
    }

    #[test]
    fn question_state_without_pending_prompt_creates_nothing() {
        let probe = DeliveryProbe::new();
        let state = question_state(Some("Which deployment region should I use?"));

        let result = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            None,
            None,
        );

        assert!(
            result.is_err(),
            "agent_state.question alone is diagnostic state, not notification authority: {result:?}"
        );
        probe.assert_silent();
    }

    // Prod oracle 4ae14e7f (2026-07-30T17:39:40.457Z): structurally-minted
    // boot-frame prompt whose question text classifies Unknown.
    #[test]
    fn unknown_subtype_question_never_notifies() {
        let probe = DeliveryProbe::new();
        let warning =
            "⚠ D:\\Developer\\codeterm\\.agents\\skills\\gap-analysis\\SKILL.md: missing field description";
        let mut state = question_state(Some(warning));
        state.subtype = Some(QuestionSubtype::Unknown);
        let pending = prompt(warning);

        let result = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            Some(&pending),
            Some(warning),
        );

        assert!(
            result.is_err(),
            "structural text with Unknown subtype is not notification authority: {result:?}"
        );
        probe.assert_silent();
    }

    #[test]
    fn question_without_classified_subtype_never_notifies() {
        let probe = DeliveryProbe::new();
        let mut state = question_state(Some("Which deployment region should I use?"));
        state.subtype = None;
        let pending = prompt("Which deployment region should I use?");

        let result = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            Some(&pending),
            None,
        );

        assert!(result.is_err(), "unclassified question fails closed: {result:?}");
        probe.assert_silent();
    }

    // Envelope/chrome text can keyword-match an actionable subtype ("model=…")
    // yet carries no actuatable control — the control gate must reject it.
    #[test]
    fn text_only_prompts_from_envelope_and_chrome_frames_never_notify() {
        let probe = DeliveryProbe::new();
        let fixtures = [
            (
                QuestionSubtype::ModelPicker,
                "-=-codeterm:tab label=Reviewer tab=166df18d model=gpt-5.6-sol-=-Coordination note only, no scope change.",
            ),
            (
                QuestionSubtype::Unknown,
                "gpt-5.6-sol high · D:\\Developer\\codeterm",
            ),
            (
                QuestionSubtype::Unknown,
                "⚠ D:\\Developer\\codeterm\\.agents\\skills\\gap-analysis\\SKILL.md: missing field description",
            ),
        ];

        for (subtype, frame_text) in fixtures {
            let mut state = question_state(Some(frame_text));
            state.subtype = Some(subtype);
            let pending = text_prompt(frame_text);
            let result = probe.admit_and_deliver(
                Some("session-owned"),
                "session-owned",
                Some(&state),
                Some(&pending),
                Some(frame_text),
            );
            assert!(
                result.is_err(),
                "text without an actuatable control never notifies ({subtype:?}): {result:?}"
            );
        }
        probe.assert_silent();
    }

    #[test]
    fn free_text_prompt_with_explicit_submit_control_admits() {
        let probe = DeliveryProbe::new();
        let state = question_state(Some("Which deployment region should I use?"));
        let mut pending = text_prompt("Which deployment region should I use?");
        pending.submit_available = Some(true);

        let result = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            Some(&pending),
            None,
        );

        assert!(result.is_ok(), "explicit submit control is actuatable: {result:?}");
        assert_eq!(probe.store.lock().list(false).len(), 1);
    }

    #[test]
    fn every_actionable_subtype_admits_a_genuine_prompt() {
        for subtype in [
            QuestionSubtype::ModelChoice,
            QuestionSubtype::ModelPicker,
            QuestionSubtype::Permission,
            QuestionSubtype::Auth,
            QuestionSubtype::Update,
        ] {
            let probe = DeliveryProbe::new();
            let mut state = question_state(Some("Which deployment region should I use?"));
            state.subtype = Some(subtype);
            let pending = prompt("Which deployment region should I use?");

            let result = probe.admit_and_deliver(
                Some("session-owned"),
                "session-owned",
                Some(&state),
                Some(&pending),
                None,
            );

            assert!(result.is_ok(), "{subtype:?} admits a genuine prompt: {result:?}");
            assert_eq!(probe.store.lock().list(false).len(), 1);
        }
    }

    #[test]
    fn genuine_pending_prompt_admits_exactly_once_across_repeated_frames() {
        let probe = DeliveryProbe::new();
        let state = question_state(Some("Which deployment region should I use?"));
        let pending = prompt("Which deployment region should I use?");

        for _ in 0..3 {
            let result = probe.admit_and_deliver(
                Some("session-owned"),
                "session-owned",
                Some(&state),
                Some(&pending),
                Some("Which deployment region should I use?"),
            );
            assert!(result.is_ok(), "authoritative prompt is admitted: {result:?}");
        }

        assert_eq!(probe.store.lock().list(false).len(), 1);
        assert_eq!(probe.native.lock().len(), 1);
        assert_eq!(probe.emitted.lock().len(), 1);
    }

    #[test]
    fn session_mismatch_stale_identity_cleared_prompt_and_paused_states_create_nothing() {
        let probe = DeliveryProbe::new();
        let state = question_state(Some("Which deployment region should I use?"));
        let pending = prompt("Which deployment region should I use?");

        let mismatched_session = probe.admit_and_deliver(
            Some("session-superseded"),
            "session-owned",
            Some(&state),
            Some(&pending),
            None,
        );
        assert!(mismatched_session.is_err());

        let stale_identity = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            Some(&pending),
            Some("An earlier prompt that was already answered"),
        );
        assert!(stale_identity.is_err());

        let cleared_prompt = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            None,
            Some("Which deployment region should I use?"),
        );
        assert!(cleared_prompt.is_err());

        let empty_prompt_question = probe.admit_and_deliver(
            Some("session-owned"),
            "session-owned",
            Some(&state),
            Some(&prompt("   ")),
            None,
        );
        assert!(empty_prompt_question.is_err());

        for subtype in [QuestionSubtype::RateLimit, QuestionSubtype::Error] {
            let mut paused = question_state(Some("Rate limited"));
            paused.subtype = Some(subtype);
            let result = probe.admit_and_deliver(
                Some("session-owned"),
                "session-owned",
                Some(&paused),
                Some(&pending),
                None,
            );
            assert!(result.is_err(), "{subtype:?} never requests input");
        }

        probe.assert_silent();
    }

    #[test]
    fn resolving_a_genuine_prompt_allows_the_next_generation_to_notify() {
        let probe = DeliveryProbe::new();
        let state = question_state(None);
        let pending = prompt("Which deployment region should I use?");

        probe
            .admit_and_deliver(
                Some("session-owned"),
                "session-owned",
                Some(&state),
                Some(&pending),
                None,
            )
            .expect("genuine prompt admits");
        probe
            .store
            .lock()
            .resolve_agent_input_request(
                "tab-owned",
                "session-owned",
                "Which deployment region should I use?",
            );
        probe
            .admit_and_deliver(
                Some("session-owned"),
                "session-owned",
                Some(&state),
                Some(&pending),
                None,
            )
            .expect("a later generation of the same prompt admits again");

        assert_eq!(probe.store.lock().list(false).len(), 2);
        assert_eq!(probe.native.lock().len(), 2);
        assert_eq!(probe.emitted.lock().len(), 2);
    }

    #[test]
    fn create_notification_delivers_bell_route_event_and_native_os_notification() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());

        let id = create_notification_with_delivery(
            NotificationAdmission::AgentInputRequest {
                session_id: "session-owned".into(),
                prompt_identity: "Which deployment region should I use?".into(),
            },
            &store,
            |title, body| {
                native
                    .lock()
                    .push((title.to_string(), body.map(str::to_string)))
            },
            "Input needed in Tab Review".into(),
            Some("Which deployment region should I use?\nInclude the failover region.".into()),
            Some("warning"),
            None,
            None,
            Some("tab-owned".into()),
            Some("workspace-owned".into()),
            |payload| emitted.lock().push(payload),
        );

        let notifications = store.lock();
        assert_eq!(notifications.list(false).len(), 1);
        let notification = notifications.list(false)[0];
        assert_eq!(notification.id, id);
        assert_eq!(notification.tab_id.as_deref(), Some("tab-owned"));
        assert_eq!(
            notification.workspace_id.as_deref(),
            Some("workspace-owned")
        );
        drop(notifications);

        assert_eq!(native.lock().len(), 1);
        let events = emitted.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], id);
        assert_eq!(events[0]["tab_id"], "tab-owned");
        assert_eq!(events[0]["workspace_id"], "workspace-owned");
        assert_eq!(
            events[0]["body"],
            "Which deployment region should I use?\nInclude the failover region."
        );
        assert!(
            events[0]["title"].as_str().unwrap().contains("Tab"),
            "user-visible notification copy uses Tab wording"
        );
    }

    #[test]
    fn replayed_input_request_skips_every_delivery_channel() {
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());

        for prompt in [
            "Which deployment region?",
            "Which deployment region?",
            "Choose a failover region",
        ] {
            create_notification_with_delivery(
                NotificationAdmission::AgentInputRequest {
                    session_id: "session-owned".into(),
                    prompt_identity: prompt.into(),
                },
                &store,
                |title, body| {
                    native
                        .lock()
                        .push((title.to_string(), body.map(str::to_string)))
                },
                "Input needed in Tab Review".into(),
                Some(prompt.into()),
                Some("warning"),
                None,
                None,
                Some("tab-owned".into()),
                Some("workspace-owned".into()),
                |payload| emitted.lock().push(payload),
            );
        }

        assert_eq!(store.lock().list(false).len(), 2);
        assert_eq!(native.lock().len(), 2);
        assert_eq!(emitted.lock().len(), 2);
    }

    #[test]
    fn diagnostic_callers_never_enter_the_agent_input_inbox() {
        let store = Arc::new(Mutex::new(NotificationStore::new(20)));
        let emitted = Mutex::new(Vec::new());
        let native = Mutex::new(Vec::new());
        let sources = [
            "service/spawn.rs:289",
            "service/spawn.rs:703",
            "cron/notify.rs",
            "cron/scheduler.rs",
            "service/orchestrator/refresh/driver.rs",
            "service/orchestrator/batch.rs",
            "api/notification_routes.rs",
            "service/output.rs",
            "service/tab.rs",
            "service/spawn_chatbackend.rs",
            "socket/server/notification.rs",
        ];

        for source in sources {
            create_notification_with_delivery(
                NotificationAdmission::Diagnostic,
                &store,
                |title, body| {
                    native
                        .lock()
                        .push((title.to_string(), body.map(str::to_string)))
                },
                format!("Diagnostic from {source}"),
                Some("warning or error detail".into()),
                Some("warning"),
                None,
                None,
                None,
                None,
                |payload| emitted.lock().push(payload),
            );
        }

        assert_eq!(store.lock().list(false).len(), 0);
        assert_eq!(emitted.lock().len(), 0);
        assert_eq!(native.lock().len(), 0);
    }
}
