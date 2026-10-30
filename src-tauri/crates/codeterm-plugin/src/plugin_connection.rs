use serde_json::Value;

use crate::agent_verbs::AgentVerbCatalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginConnectionState {
    SignedIn,
    SignedOut,
    RuntimeMissing,
    Pending,
    Unknown,
}

impl PluginConnectionState {
    pub fn from_response(response: &Value) -> Option<Self> {
        if let Some(state) = response
            .get("error")
            .and_then(Value::as_str)
            .and_then(Self::from_error)
        {
            return Some(state);
        }
        if let Some(result) = response.get("result").and_then(Value::as_str) {
            if let Some(state) = serde_json::from_str(result)
                .ok()
                .and_then(|value| Self::from_response(&value))
            {
                return Some(state);
            }
        }
        match response.get("state").and_then(Value::as_str).unwrap_or("") {
            "logged-in" => Some(Self::SignedIn),
            "logged-out"
            | "not-logged-in"
            | "reauth-needed"
            | "installed-but-not-configured"
            | "expired"
            | "refused" => Some(Self::SignedOut),
            "not-installed" => Some(Self::RuntimeMissing),
            "awaiting-user" | "install-in-progress" | "password-required" => Some(Self::Pending),
            _ => None,
        }
    }

    pub fn from_error(error: &str) -> Option<Self> {
        match error.split(':').next()?.trim() {
            "not-logged-in" | "reauth-needed" => Some(Self::SignedOut),
            "not-installed" => Some(Self::RuntimeMissing),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SignedIn => "signed in",
            Self::SignedOut => "not signed in",
            Self::RuntimeMissing => "client not installed; not signed in",
            Self::Pending => "sign-in pending",
            Self::Unknown => "state unknown",
        }
    }
}

pub fn login_command(catalog: &AgentVerbCatalog) -> Option<String> {
    catalog
        .verbs
        .iter()
        .any(|usage| usage == "login")
        .then(|| format!("domios plugin {} login", catalog.id))
}

pub fn actionable_refusal(catalog: &AgentVerbCatalog, error: &str) -> String {
    let Some(command) =
        login_command(catalog).filter(|_| PluginConnectionState::from_error(error).is_some())
    else {
        return error.to_owned();
    };
    format!(
        "{error}\nNext: `{command}`. Offer chat-first sign-in; show the returned sign-in code or QR and instructions to the user in chat. Setup requirements: `domios plugin get {}`.",
        catalog.id
    )
}

pub fn recall_line(catalog: &AgentVerbCatalog) -> String {
    let help = format!("domios plugin {} --help", catalog.id);
    let Some(login) = login_command(catalog) else {
        return format!("{}: `{help}`.", catalog.display_name);
    };
    let state = catalog
        .connection_state
        .unwrap_or(PluginConnectionState::Unknown);
    let next = match state {
        PluginConnectionState::SignedIn => format!("`{help}`"),
        PluginConnectionState::Unknown | PluginConnectionState::Pending => {
            return format!(
                "{}: {} — run `domios plugin {} login-status`.",
                catalog.display_name,
                state.label(),
                catalog.id
            );
        }
        _ => format!("Offer sign-in: `{login}`"),
    };
    format!("{}: {}. {next}.", catalog.display_name, state.label())
}

#[cfg(test)]
mod tests;
