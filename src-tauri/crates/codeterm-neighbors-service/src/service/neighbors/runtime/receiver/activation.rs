use codeterm_neighbors::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub transient: bool,
    pub kind: &'static str,
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        let (transient, kind) = match error {
            Error::Transport => (true, "transport"),
            Error::Unavailable => (true, "backend_unavailable"),
            Error::EntitlementsUnavailable => (true, "entitlements_unavailable"),
            Error::KeyInventoryPending => (true, "key_inventory_pending"),
            Error::SecretStore => (true, "secret_store_unavailable"),
            Error::CursorStore => (true, "metadata_unavailable"),
            Error::Http {
                status: 408 | 429 | 500..=599,
                ..
            } => (true, "http_unavailable"),
            Error::Firestore {
                code: 1 | 2 | 4 | 8 | 10 | 13 | 14,
                ..
            } => (true, "firestore_unavailable"),
            Error::LocalKeyUnavailable | Error::KeyNotFound | Error::InvalidKey => {
                (false, "key_unavailable")
            }
            Error::Revoked | Error::SessionRevoked => (false, "revoked"),
            Error::NotReceiver
            | Error::ReceiverElsewhere
            | Error::ReceivingElsewhere
            | Error::ReceiverRevisionStale => (false, "receiver_changed"),
            _ => (false, "reconcile_refused"),
        };
        Self { transient, kind }
    }
}

#[cfg(test)]
mod tests;

impl From<String> for Failure {
    fn from(_: String) -> Self {
        Self {
            transient: false,
            kind: "reconcile_refused",
        }
    }
}

impl From<&str> for Failure {
    fn from(_: &str) -> Self {
        Self {
            transient: false,
            kind: "reconcile_refused",
        }
    }
}

#[derive(Default)]
pub struct State {
    pub generation: u64,
    pub failure: Option<Failure>,
}

impl State {
    pub fn transition(&mut self, generation: u64, failure: Option<Failure>) -> bool {
        let changed = self.generation != generation || self.failure != failure;
        self.generation = generation;
        self.failure = failure;
        changed
    }
}
