use super::callable::Calls;
use codeterm_account::firebase_endpoints::{self, Endpoints, PRODUCTION};
use codeterm_neighbors::{
    adapter::{AuthenticatedCall, FirebaseTransport, FirestoreTarget},
    Error, Result,
};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Selection {
    endpoints: Endpoints,
    firestore: FirestoreTarget,
}

impl Selection {
    fn resolve(debug: bool, endpoints: Endpoints) -> Result<Self> {
        let firestore = if endpoints.emulator {
            if !debug || endpoints.qa
                || endpoints.identity_toolkit != "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/"
                || endpoints.secure_token != "http://127.0.0.1:19099/securetoken.googleapis.com/v1/"
                || endpoints.functions != "http://127.0.0.1:15001/demo-domios/us-central1/"
                || endpoints.signin != "http://127.0.0.1:15000/signin/"
                || endpoints.redeem != "http://127.0.0.1:15001/demo-domios/us-central1/redeemDesktopLoginCode"
            {
                return Err(Error::Transport);
            }
            FirestoreTarget::debug_emulator("http://127.0.0.1:18080", "demo-domios")?
        } else if endpoints.qa {
            let qa = Endpoints {
                functions: "https://us-central1-domios-signin-qa.cloudfunctions.net/",
                signin: "https://domios-signin-qa.web.app/signin/",
                redeem: "https://us-central1-domios-signin-qa.cloudfunctions.net/redeemDesktopLoginCode",
                qa: true,
                ..PRODUCTION
            };
            if !debug || endpoints != qa {
                return Err(Error::Transport);
            }
            FirestoreTarget::google("domios-signin-qa")?
        } else {
            if endpoints != PRODUCTION {
                return Err(Error::Transport);
            }
            FirestoreTarget::google("domios-d9357")?
        };
        Ok(Self { endpoints, firestore })
    }

    fn current() -> Result<Self> {
        Self::resolve(cfg!(debug_assertions), firebase_endpoints::current())
    }

    fn validate(&self, current: Endpoints) -> Result<()> {
        if self.endpoints == current {
            Ok(())
        } else {
            Err(Error::Revoked)
        }
    }

    fn transport(
        &self,
        calls: Arc<dyn AuthenticatedCall>,
        uid: String,
        installation_id: String,
    ) -> Result<Arc<FirebaseTransport>> {
        if calls.firestore_target(self.firestore.project())? != self.firestore {
            return Err(Error::Transport);
        }
        Ok(Arc::new(FirebaseTransport {
            calls,
            project: self.firestore.project().into(),
            uid,
            installation_id,
        }))
    }
}

pub(in crate::service::neighbors) fn calls(calls: Calls) -> Result<Arc<dyn AuthenticatedCall>> {
    bind(calls, firebase_endpoints::current(), firebase_endpoints::current)
}

pub(super) fn bind(
    calls: Calls,
    endpoints: Endpoints,
    current: fn() -> Endpoints,
) -> Result<Arc<dyn AuthenticatedCall>> {
    Ok(Arc::new(SelectedCalls {
        calls,
        selection: Selection::resolve(cfg!(debug_assertions), endpoints)?,
        current,
    }))
}

pub(in crate::service::neighbors) fn new(
    calls: Arc<dyn AuthenticatedCall>,
    uid: String,
    installation_id: String,
) -> Result<Arc<FirebaseTransport>> {
    Selection::current()?.transport(calls, uid, installation_id)
}

struct SelectedCalls {
    calls: Calls,
    selection: Selection,
    current: fn() -> Endpoints,
}

#[async_trait::async_trait]
impl AuthenticatedCall for SelectedCalls {
    fn firestore_target(&self, project: &str) -> Result<FirestoreTarget> {
        self.selection.validate((self.current)())?;
        if project != self.selection.firestore.project() {
            return Err(Error::Transport);
        }
        Ok(self.selection.firestore.clone())
    }

    fn firestore_failure(&self, operation: &'static str, target: &str, code: Option<i32>, kind: &str) {
        self.calls.firestore_failure(operation, target, code, kind);
    }

    async fn id_token(&self) -> Result<zeroize::Zeroizing<String>> {
        self.selection.validate((self.current)())?;
        let token = self.calls.id_token().await?;
        self.selection.validate((self.current)())?;
        Ok(token)
    }

    async fn invoke(&self, route: &str, data: serde_json::Value) -> Result<serde_json::Value> {
        self.selection.validate((self.current)())?;
        let result = self.calls.invoke_at(self.selection.endpoints.functions, route, data).await;
        self.selection.validate((self.current)())?;
        result
    }
}

#[cfg(test)]
mod tests;
