//! The `mem secret save-master-password` decision, kept pure so its contract is
//! testable without a daemon, a vault, or a real credential.

/// The one notice that asserts auto-unlock, emitted only on the path that earned
/// it, so tests assert its absence by identifier rather than by sentence.
pub const AUTO_UNLOCK_ARMED_NOTICE: &str =
    "Auto-unlock armed: the active backend restores its own session without a prompt.";

/// What the active backend said about itself, read from `/api/secrets/status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDeclaration {
    pub backend: String,
    pub auto_unlock_capable: bool,
}

impl BackendDeclaration {
    pub fn from_status(body: &serde_json::Value) -> Self {
        Self {
            backend: body
                .get("backend")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            auto_unlock_capable: body
                .get("auto_unlock_capable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    }
}

/// The only structure in this command that carries the credential, and the only
/// caller that sets the consent bit. It lives beside the emitters so a test can
/// prove the two are disjoint.
pub fn arm_unlock_request(master_password: &str) -> serde_json::Value {
    serde_json::json!({
        "master_password": master_password,
        "persist_for_auto_unlock": true,
    })
}

/// Named outcomes. The discriminant is the assertion surface; no variant ever
/// carries credential material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveMasterPasswordOutcome {
    /// The active backend declares no auto-unlock capability. Nothing was sent,
    /// nothing was stored, and nothing is claimed.
    CapabilityNotDeclared { backend: String },
    /// The backend declares the capability but the vault refused the credential.
    VaultRefused { backend: String, http_status: u16 },
    /// The vault accepted the credential but the backend did not arm itself.
    NotArmed { backend: String },
    /// The vault accepted the credential and the backend armed auto-unlock.
    Armed { backend: String },
}

impl SaveMasterPasswordOutcome {
    /// Decide from the two observations this command can actually make: what the
    /// backend declared, and what the vault and post-unlock status reported.
    pub fn decide(
        declaration: &BackendDeclaration,
        unlock_status: Option<u16>,
        armed_after: bool,
    ) -> Self {
        let backend = declaration.backend.clone();
        if !declaration.auto_unlock_capable {
            return Self::CapabilityNotDeclared { backend };
        }
        match unlock_status {
            Some(200 | 201 | 204) => {
                if armed_after {
                    Self::Armed { backend }
                } else {
                    Self::NotArmed { backend }
                }
            }
            Some(http_status) => Self::VaultRefused {
                backend,
                http_status,
            },
            None => Self::VaultRefused {
                backend,
                http_status: 0,
            },
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Armed { .. })
    }

    /// Locked / logged-out / unavailable keep the documented script codes so
    /// existing caller branches still hold.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Armed { .. } => 0,
            Self::CapabilityNotDeclared { .. } => 2,
            Self::NotArmed { .. } => 1,
            Self::VaultRefused { http_status, .. } => match http_status {
                401 => 4,
                423 => 3,
                404 => 5,
                503 | 0 => 6,
                _ => 1,
            },
        }
    }

    /// Everything the command prints in human mode. Describes only what happened.
    pub fn lines(&self) -> Vec<String> {
        match self {
            Self::CapabilityNotDeclared { backend } => vec![
                format!("Backend '{backend}' does not declare the auto-unlock capability."),
                "Nothing was stored. Run `codeterm mem secret unlock` when the session expires."
                    .to_string(),
            ],
            Self::VaultRefused {
                backend,
                http_status,
            } => vec![format!(
                "Backend '{backend}' did not accept the credential (HTTP {http_status}). Nothing was stored."
            )],
            Self::NotArmed { backend } => vec![
                format!("Backend '{backend}' accepted the credential but stored nothing."),
                "Auto-unlock is not armed; a manual unlock is still required.".to_string(),
            ],
            Self::Armed { backend } => vec![
                format!("Backend '{backend}' accepted the credential and stored it in its own bucket."),
                AUTO_UNLOCK_ARMED_NOTICE.to_string(),
            ],
        }
    }
}

/// Everything the command prints in `--json` mode.
pub fn json_summary(
    declaration: &BackendDeclaration,
    outcome: &SaveMasterPasswordOutcome,
) -> serde_json::Value {
    serde_json::json!({
        "backend": declaration.backend,
        "auto_unlock_capable": declaration.auto_unlock_capable,
        "auto_unlock_armed": outcome.is_success(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Obviously not a credential. Supplied only as credential data, never as an
    // identity, so an appearance in output is a real leak.
    const SYNTHETIC_SECRET: &str = "synthetic-not-a-credential-0000";

    fn declaring(backend: &str) -> BackendDeclaration {
        BackendDeclaration {
            backend: backend.to_string(),
            auto_unlock_capable: true,
        }
    }

    fn silent(backend: &str) -> BackendDeclaration {
        BackendDeclaration {
            backend: backend.to_string(),
            auto_unlock_capable: false,
        }
    }

    fn every_outcome() -> Vec<(BackendDeclaration, SaveMasterPasswordOutcome)> {
        let mut out = Vec::new();
        for declaration in [declaring("alpha-vault"), silent("alpha-vault")] {
            for status in [Some(200u16), Some(401), Some(423), Some(503), None] {
                for armed in [true, false] {
                    let outcome = SaveMasterPasswordOutcome::decide(&declaration, status, armed);
                    out.push((declaration.clone(), outcome));
                }
            }
        }
        out
    }

    #[test]
    fn save_master_password_does_not_promise_absent_autounlock() {
        let outcome = SaveMasterPasswordOutcome::decide(&silent("some-vault"), None, false);
        assert_eq!(
            outcome,
            SaveMasterPasswordOutcome::CapabilityNotDeclared {
                backend: "some-vault".to_string()
            }
        );
        // The promise constant appears on no line of the path that did not earn it.
        for line in outcome.lines() {
            assert!(!line.contains(AUTO_UNLOCK_ARMED_NOTICE), "line: {line}");
        }
        assert!(!outcome.is_success());
        assert_ne!(outcome.exit_code(), 0);
    }

    #[test]
    fn declaring_a_capability_does_not_authorize_the_vault() {
        // Declaring auto-unlock and still being refused: the capability decides
        // what is offered, the vault decides what is permitted.
        let refused =
            SaveMasterPasswordOutcome::decide(&declaring("declaring-vault"), Some(401), false);
        assert_eq!(
            refused,
            SaveMasterPasswordOutcome::VaultRefused {
                backend: "declaring-vault".to_string(),
                http_status: 401,
            }
        );
        assert_eq!(refused.exit_code(), 4);
        for line in refused.lines() {
            assert!(!line.contains(AUTO_UNLOCK_ARMED_NOTICE));
        }

        let locked =
            SaveMasterPasswordOutcome::decide(&declaring("declaring-vault"), Some(423), false);
        assert_eq!(locked.exit_code(), 3);
        assert!(!locked.is_success());
    }

    #[test]
    fn a_declaring_backend_that_stores_nothing_is_reported_honestly() {
        let outcome =
            SaveMasterPasswordOutcome::decide(&declaring("declaring-vault"), Some(200), false);
        assert_eq!(
            outcome,
            SaveMasterPasswordOutcome::NotArmed {
                backend: "declaring-vault".to_string()
            }
        );
        assert_ne!(outcome.exit_code(), 0);
        for line in outcome.lines() {
            assert!(!line.contains(AUTO_UNLOCK_ARMED_NOTICE));
        }
    }

    #[test]
    fn only_an_armed_declaring_backend_earns_the_notice() {
        let outcome =
            SaveMasterPasswordOutcome::decide(&declaring("declaring-vault"), Some(200), true);
        assert!(outcome.is_success());
        assert_eq!(outcome.exit_code(), 0);
        assert!(outcome
            .lines()
            .iter()
            .any(|l| l.contains(AUTO_UNLOCK_ARMED_NOTICE)));
    }

    #[test]
    fn two_backends_declaring_the_same_capability_are_treated_identically() {
        for status in [Some(200u16), Some(401), None] {
            for armed in [true, false] {
                let a = SaveMasterPasswordOutcome::decide(&declaring("alpha-vault"), status, armed);
                let b = SaveMasterPasswordOutcome::decide(&declaring("bitwarden"), status, armed);
                assert_eq!(a.exit_code(), b.exit_code());
                assert_eq!(a.is_success(), b.is_success());
                assert_eq!(
                    std::mem::discriminant(&a),
                    std::mem::discriminant(&b),
                    "identical declarations must take identical paths"
                );
            }
        }
    }

    #[test]
    fn the_arm_request_is_the_only_carrier_of_the_credential() {
        let request = arm_unlock_request(SYNTHETIC_SECRET);
        // The secret really is in flight here, so its absence elsewhere is meaningful.
        assert!(request.to_string().contains(SYNTHETIC_SECRET));
        assert_eq!(request["persist_for_auto_unlock"], serde_json::json!(true));
    }

    #[test]
    fn no_emitted_line_can_carry_the_credential() {
        let _in_flight = arm_unlock_request(SYNTHETIC_SECRET);
        for (declaration, outcome) in every_outcome() {
            for line in outcome.lines() {
                assert!(
                    !line.contains(SYNTHETIC_SECRET),
                    "human output leaked the credential: {line}"
                );
            }
            let rendered = json_summary(&declaration, &outcome).to_string();
            assert!(
                !rendered.contains(SYNTHETIC_SECRET),
                "json output leaked the credential: {rendered}"
            );
        }
    }

    #[test]
    fn the_decision_never_receives_credential_material() {
        // The outcome type and its emitters sit after the request builder and
        // must never name the credential, which is why none can render one.
        let emitters = include_str!("mem_secret_autounlock.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("module has a non-test half")
            .split("/// Named outcomes.")
            .nth(1)
            .expect("module defines the outcomes")
            .to_string();
        assert!(!emitters.contains("master_password"));
    }
}
