use serde_json::{json, Value};

/// Caller identity resolved once from the environment, applied per call site.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CallerContext {
    tab_id: Option<String>,
    session_id: Option<String>,
    session_binding_nonce: Option<String>,
    tab_title: Option<String>,
}

impl CallerContext {
    pub(crate) fn from_env() -> Self {
        Self {
            tab_id: crate::nonempty_env("CODETERM_TAB_ID"),
            session_id: crate::nonempty_env("CODETERM_SESSION_ID"),
            session_binding_nonce: crate::nonempty_env("CODETERM_SESSION_BINDING_NONCE"),
            tab_title: crate::nonempty_env("CODETERM_TAB_TITLE"),
        }
    }

    pub(crate) fn tab_id(&self) -> Option<String> {
        self.tab_id.clone()
    }

    /// All three keys stay present with JSON null when unset; a daemon-side
    /// `has_key` check would flip if absent fields replaced the nulls.
    pub(crate) fn owner(&self) -> Value {
        json!({
            "tab_id": self.tab_id,
            "session_id": self.session_id,
            "title": self.tab_title,
        })
    }

    pub(crate) fn apply_caller_tab(&self, params: &mut Value) {
        let Some(tab) = &self.tab_id else {
            return;
        };
        if let Some(obj) = params.as_object_mut() {
            obj.insert("caller_tab_id".into(), json!(tab));
        }
    }

    pub(crate) fn apply_trusted_session(&self, params: &mut Value) {
        let Some(obj) = params.as_object_mut() else {
            return;
        };
        if let Some(tab_id) = &self.tab_id {
            obj.insert("caller_tab_id".into(), json!(tab_id));
        }
        if let Some(session_id) = &self.session_id {
            obj.insert("session_id".into(), json!(session_id));
        }
        if let Some(nonce) = &self.session_binding_nonce {
            obj.insert("session_binding_nonce".into(), json!(nonce));
        }
    }

    pub(crate) fn report_body(&self, tab_id: &str, message: String) -> Value {
        let mut body = json!({
            "tab_id": tab_id,
            "caller_tab_id": tab_id,
            "message": message,
        });
        if let Some(session) = &self.session_id {
            body["caller_session_id"] = json!(session);
        }
        if let Some(nonce) = &self.session_binding_nonce {
            body["session_binding_nonce"] = json!(nonce);
        }
        body
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) fn test_utility_caller() -> CallerContext {
    CallerContext {
        tab_id: Some("utility".into()),
        session_binding_nonce: Some("utility-nonce".into()),
        ..Default::default()
    }
}
