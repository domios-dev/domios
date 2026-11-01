use crate::service::{account::AccountCell, ServiceContext};
use codeterm_account::{account_mesh::callable::CallableGuard, Session};

pub(crate) struct Changes(pub tokio::sync::watch::Sender<u64>);

impl Default for Changes {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(0).0)
    }
}

impl ServiceContext {
    pub(crate) fn callable_admission_changes(&self) -> tokio::sync::watch::Sender<u64> {
        self.extensions.get_or_init(Changes::default).0.clone()
    }
    pub(crate) fn retire_callable_waiters(&self) {
        self.callable_admission_changes()
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

pub(crate) struct SessionGuard<'a> {
    pub service: &'a ServiceContext,
    pub expected: &'a Session,
    pub neighbors: bool,
}

#[async_trait::async_trait]
impl CallableGuard for SessionGuard<'_> {
    async fn retired(&self) {
        let mut changes = self.service.callable_admission_changes().subscribe();
        loop {
            if !self.current().await {
                return;
            }
            if changes.changed().await.is_err() {
                return;
            }
        }
    }

    async fn current(&self) -> bool {
        if self.neighbors && self.service.require_neighbors_enabled().is_err() {
            return false;
        }
        let Some(account) = self.service.ext::<AccountCell>().0.get() else {
            return false;
        };
        account.session().await.is_some_and(|session| {
            session.identity.uid == self.expected.identity.uid
                && session.login_generation == self.expected.login_generation
        })
    }
}

#[cfg(test)]
mod tests;
