use std::collections::HashMap;

use parking_lot::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabCaller {
    pub tab_id: String,
    pub binding_nonce: String,
}

impl TabCaller {
    pub fn validate_claim(&self, claimed: Option<&str>) -> Result<(), String> {
        if claimed.is_some_and(|claimed| claimed != self.tab_id) {
            return Err("Caller tab does not match its credential".into());
        }
        Ok(())
    }

    pub fn bind_rpc(&self, method: &str, params: &mut Option<serde_json::Value>) -> Result<(), String> {
        let body = params.get_or_insert_with(|| serde_json::json!({}));
        if body.is_null() { *body = serde_json::json!({}); }
        let body = body.as_object_mut().ok_or("RPC params must be an object")?;
        for field in ["caller_tab_id", "source_tab", "creator_tab_id", "spawned_by_tab_id", "asked_by_tab_id", "answered_by_tab_id"] {
            if let Some(value) = body.get(field).filter(|value| !value.is_null()) {
                let claim = value.as_str().ok_or("Invalid caller tab claim")?;
                self.validate_claim(Some(claim))?;
            }
        }
        if method.starts_with("model_provider.") || method.starts_with("system.") || method.starts_with("rest_api.") {
            return Ok(());
        }
        body.insert("caller_tab_id".into(), self.tab_id.clone().into());
        if matches!(method, "agent.ack" | "agent.report" | "agent.diagnostic") {
            if let Some(value) = body.get("tab_id").filter(|value| !value.is_null()) {
                self.validate_claim(Some(value.as_str().ok_or("Invalid actor tab")?))?;
            }
            body.insert("tab_id".into(), self.tab_id.clone().into());
        }
        if method == "tab.send_text" {
            if body.get("source_peer").is_some_and(|value| !value.is_null()) || body.get("system_context").is_some_and(|value| !value.is_null()) {
                return Err("Tab credentials cannot supply peer or system provenance".into());
            }
            body.insert("source_tab".into(), self.tab_id.clone().into());
        }
        if matches!(method, "tab.create" | "tab.new" | "cron.create" | "agent.spawn") {
            body.insert("creator_tab_id".into(), self.tab_id.clone().into());
        }
        if method == "plan.qa.next" {
            if let Some(claim) = body.get("qa_tab_id").filter(|value| !value.is_null()) {
                self.validate_claim(Some(claim.as_str().ok_or("Invalid QA caller claim")?))?;
            }
        }
        if matches!(method, "agent.spawn" | "orchestration.new") {
            if body.get("origin_peer").is_some_and(|value| !value.is_null()) {
                return Err("Tab credentials cannot supply peer provenance".into());
            }
            body.insert("spawned_by_tab_id".into(), self.tab_id.clone().into());
        }
        for field in ["session_binding_nonce", "caller_binding_nonce"] {
            if body.get(field).filter(|value| !value.is_null()).is_some_and(|value| value.as_str() != Some(self.binding_nonce.as_str())) {
                return Err("Caller nonce does not match its credential".into());
            }
        }
        body.insert("session_binding_nonce".into(), self.binding_nonce.clone().into());
        if method == "agent.startup_ack" {
            if body.get("binding_nonce").is_some_and(|value| value.as_str() != Some(self.binding_nonce.as_str())) {
                return Err("Startup nonce does not match its credential".into());
            }
            body.insert("binding_nonce".into(), self.binding_nonce.clone().into());
        }
        Ok(())
    }
}

struct Credential {
    token: String,
    caller: TabCaller,
    session: Option<String>,
}

#[derive(Default)]
pub(crate) struct TabCredentials(Mutex<HashMap<String, Credential>>);

impl TabCredentials {
    fn mint(&self, caller: TabCaller) -> String {
        let mut credentials = self.0.lock();
        let entry = credentials.entry(caller.tab_id.clone()).or_insert_with(|| {
            Credential {token: uuid::Uuid::new_v4().to_string(), caller: caller.clone(), session: None}
        });
        if entry.caller != caller {
            *entry = Credential {token: uuid::Uuid::new_v4().to_string(), caller, session: None};
        }
        entry.token.clone()
    }

    fn identify(&self, token: &str) -> Option<TabCaller> {
        self.0.lock().values().find_map(|credential| {
            super::tokens::constant_time_eq(credential.token.as_bytes(), token.as_bytes())
                .then(|| credential.caller.clone())
        })
    }

    fn verify_session(&self, token: &str, current: Option<&str>, attested: bool) -> bool {
        let mut credentials = self.0.lock();
        let Some((tab_id, credential)) = credentials.iter_mut().find(|(_, credential)| {
            super::tokens::constant_time_eq(credential.token.as_bytes(), token.as_bytes())
        }) else { return false; };
        if let Some(session) = &credential.session {
            let matches = Some(session.as_str()) == current;
            if !matches {
                let tab_id = tab_id.clone();
                credentials.remove(&tab_id);
            }
            return matches;
        }
        if attested { credential.session = current.map(str::to_owned); }
        true
    }

    fn revoke_token(&self, token: &str) {
        self.0.lock().retain(|_, credential| {
            !super::tokens::constant_time_eq(credential.token.as_bytes(), token.as_bytes())
        });
    }

    fn revoke(&self, tab_id: &str) {
        self.0.lock().remove(tab_id);
    }
}

impl super::ServiceContext {
    pub fn authenticate_local_credential(&self, token: &str) -> Result<Option<TabCaller>, String> {
        if token.is_empty() {
            return Err("A local API credential is required".into());
        }
        if let Some(caller) = self.identify_tab_credential(token) {
            return Ok(Some(caller));
        }
        if self.ext::<crate::config::ConfigState>().lock().api.token.as_ref().is_some_and(|expected| {
            !expected.is_empty() && super::tokens::constant_time_eq(expected.as_bytes(), token.as_bytes())
        }) {
            return Ok(None);
        }
        Err("Invalid or revoked local API credential".into())
    }

    pub fn bind_local_rpc(&self, token: &str, method: &str, params: &mut Option<serde_json::Value>) -> Result<(), String> {
        match self.authenticate_local_credential(token)? {
            Some(caller) => {
                if matches!(method, "mem.worker.report" | "mem.worker.progress") {
                    let worker_id = params.as_ref().and_then(|body| body.get("id")).and_then(serde_json::Value::as_str).ok_or("Worker id is required")?;
                    if !self.workers.get(worker_id).is_some_and(|worker| worker.tab_id == caller.tab_id) {
                        return Err("Worker reports require the owning tab credential".into());
                    }
                }
                caller.bind_rpc(method, params)?;
                let session = self.mux.read().get_tab(&caller.tab_id).and_then(|tab| tab.agent_session_id.clone());
                let body = params.as_mut().and_then(serde_json::Value::as_object_mut).ok_or("RPC params must be an object")?;
                if let Some(claim) = body.get("caller_session_id").filter(|value| !value.is_null()) {
                    if claim.as_str() != session.as_deref() {
                        return Err("Caller session does not match its credential".into());
                    }
                }
                for field in ["from", "owner"] {
                    if field == "owner" && method != "plan.create" { continue; }
                    if let Some(selected) = body.get(field).filter(|value| !value.is_null()) {
                        let selected: crate::plan::AgentRef = serde_json::from_value(selected.clone()).map_err(|_| "Invalid actor reference")?;
                        caller.validate_claim(selected.tab_id.as_deref())?;
                        let title = self.mux.read().get_tab(&caller.tab_id).map(|tab| tab.title.clone()).ok_or("Caller tab has closed")?;
                        let actor = crate::plan::AgentRef {tab_id: Some(caller.tab_id.clone()), session_id: session.clone(), title: Some(title)};
                        body.insert(field.into(), serde_json::to_value(actor).map_err(|error| error.to_string())?);
                    }
                }
                if method == "plan.batch" {
                    if let Some(ops) = body.get_mut("ops").and_then(serde_json::Value::as_array_mut) {
                        for op in ops {
                            if matches!(op.get("op").and_then(serde_json::Value::as_str), Some("add_milestone" | "add_handoff")) {
                                if let Some(selected) = op.get("from").filter(|value| !value.is_null()) {
                                    let selected: crate::plan::AgentRef = serde_json::from_value(selected.clone()).map_err(|_| "Invalid batch actor reference")?;
                                    caller.validate_claim(selected.tab_id.as_deref())?;
                                }
                                let actor = crate::plan::AgentRef {tab_id: Some(caller.tab_id.clone()), session_id: session.clone(), title: None};
                                op["from"] = serde_json::to_value(actor).map_err(|error| error.to_string())?;
                            }
                        }
                    }
                }
                if let Some(session) = session.filter(|_| matches!(method, "agent.report" | "agent.ack")) {
                    body.insert("caller_session_id".into(), session.into());
                }
            }
            None => {
                if params.as_ref().and_then(serde_json::Value::as_object).is_some_and(|body| {
                    ["caller_tab_id", "source_tab", "creator_tab_id", "spawned_by_tab_id", "asked_by_tab_id", "answered_by_tab_id"]
                        .iter().any(|field| body.get(*field).is_some_and(|value| !value.is_null()))
                }) {
                    return Err("Tab caller claims require a tab credential".into());
                }
                if matches!(method, "agent.ack" | "agent.report" | "agent.startup_ack") {
                    return Err("Agent acknowledgements and reports require a tab credential".into());
                }
            }
        }
        if method == "api.request" {
            let body = params.as_mut().and_then(serde_json::Value::as_object_mut).ok_or("RPC params must be an object")?;
            body.insert("authenticated_api_credential".into(), token.into());
        }
        Ok(())
    }

    pub(crate) fn mint_tab_credential(&self, tab_id: &str, expected_nonce: &str) -> Option<String> {
        let mux = self.mux.read();
        let tab = mux.get_tab(tab_id)?;
        let binding_nonce = tab.session_binding_nonce.as_ref().filter(|nonce| !nonce.is_empty())?;
        if binding_nonce.as_str() != expected_nonce { return None; }
        Some(self.tab_credentials.mint(TabCaller {
            tab_id: tab.id.clone(),
            binding_nonce: binding_nonce.clone(),
        }))
    }

    pub fn identify_tab_credential(&self, token: &str) -> Option<TabCaller> {
        let caller = self.tab_credentials.identify(token)?;
        let mux = self.mux.read();
        let Some(tab) = mux.get_tab(&caller.tab_id) else {
            self.tab_credentials.revoke_token(token);
            return None;
        };
        let current_launch = tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str());
        let attested = tab.reported_session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str());
        if !current_launch {
            self.tab_credentials.revoke_token(token);
            return None;
        }
        (self.tab_credentials.verify_session(token, tab.agent_session_id.as_deref(), attested))
            .then_some(caller)
    }

    pub(crate) fn revoke_tab_credential(&self, tab_id: &str) {
        self.tab_credentials.revoke(tab_id);
    }
}

#[cfg(test)]
mod tests;
