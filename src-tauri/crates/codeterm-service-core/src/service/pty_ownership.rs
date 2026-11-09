use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct PtyOwnership {
    launch_claims: Arc<Mutex<HashMap<String, String>>>,
    #[cfg(any(test, feature = "test-support"))]
    observer: Arc<Mutex<Option<OwnershipObserver>>>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnershipEvent {
    AwaitingActivation,
    Linking {
        candidate: String,
        launch_owned: bool,
    },
}

#[cfg(any(test, feature = "test-support"))]
pub type OwnershipObserver = Arc<dyn Fn(&str, OwnershipEvent) + Send + Sync>;

pub struct TabPtyClaim {
    tab_id: String,
    ownership: PtyOwnership,
    _activation: crate::service::pty_activation::TabActivationPermit,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PtySelection {
    pub pty_id: String,
    pub retained_candidate: bool,
    pub linked: bool,
}

impl PtySelection {
    pub fn reader_pty_id(&self) -> Option<&str> {
        (self.linked && self.retained_candidate).then_some(self.pty_id.as_str())
    }
}

pub enum LinkOutcome {
    Linked(Option<String>),
    TabMissing,
}

impl PtyOwnership {
    pub fn acquire(&self, tab_id: &str) -> Result<TabPtyClaim, String> {
        Ok(TabPtyClaim {
            tab_id: tab_id.to_string(),
            ownership: self.clone(),
            _activation: self.activate(tab_id)?,
        })
    }

    #[cfg(not(any(test, feature = "test-support")))]
    fn activate(
        &self,
        tab_id: &str,
    ) -> Result<crate::service::pty_activation::TabActivationPermit, String> {
        crate::service::pty_activation::acquire(tab_id)
    }

    #[cfg(any(test, feature = "test-support"))]
    fn activate(
        &self,
        tab_id: &str,
    ) -> Result<crate::service::pty_activation::TabActivationPermit, String> {
        crate::service::pty_activation::acquire_with_wait_hook(tab_id, || {
            self.observe(tab_id, OwnershipEvent::AwaitingActivation)
        })
    }

    pub async fn acquire_async(&self, tab_id: String) -> Result<TabPtyClaim, String> {
        #[cfg(not(any(test, feature = "test-support")))]
        let activation = crate::service::pty_activation::acquire_async(tab_id.clone()).await?;
        #[cfg(any(test, feature = "test-support"))]
        let activation =
            crate::service::pty_activation::acquire_async_with_wait_hook(tab_id.clone(), || {
                self.observe(&tab_id, OwnershipEvent::AwaitingActivation)
            })
            .await?;
        Ok(TabPtyClaim {
            tab_id,
            ownership: self.clone(),
            _activation: activation,
        })
    }

    #[cfg(test)]
    fn acquire_with_wait_hook(&self, tab_id: &str, waiting: impl FnMut()) -> TabPtyClaim {
        TabPtyClaim {
            tab_id: tab_id.to_string(),
            ownership: self.clone(),
            _activation: crate::service::pty_activation::acquire_with_wait_hook(tab_id, waiting)
                .unwrap(),
        }
    }

    pub fn release_pty(&self, pty_id: &str) {
        self.launch_claims
            .lock()
            .retain(|_, claimed| claimed != pty_id);
    }

    pub fn release_ptys(&self, pty_ids: &[String]) {
        if pty_ids.is_empty() {
            return;
        }
        self.launch_claims
            .lock()
            .retain(|_, claimed| !pty_ids.contains(claimed));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_test_observer(&self, observer: OwnershipObserver) {
        *self.observer.lock() = Some(observer);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn observe(&self, tab_id: &str, event: OwnershipEvent) {
        let observer = self.observer.lock().clone();
        if let Some(observer) = observer {
            observer(tab_id, event);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn launch_claim(&self, tab_id: &str) -> Option<String> {
        self.launch_claims.lock().get(tab_id).cloned()
    }
}

impl TabPtyClaim {
    pub fn tab_id(&self) -> &str {
        &self.tab_id
    }

    pub fn settle(
        &self,
        candidate_pty_id: String,
        launch_owned: bool,
        mut is_live: impl FnMut(&str) -> bool,
        mut link: impl FnMut(&str) -> LinkOutcome,
        mut kill: impl FnMut(&str),
        mut ready: impl FnMut(&str),
    ) -> PtySelection {
        #[cfg(any(test, feature = "test-support"))]
        self.ownership.observe(
            &self.tab_id,
            OwnershipEvent::Linking {
                candidate: candidate_pty_id.clone(),
                launch_owned,
            },
        );
        let existing_claim = {
            let claims = self.ownership.launch_claims.lock();
            claims.get(&self.tab_id).cloned()
        };
        let mut claimed = existing_claim.and_then(|claimed| {
            if is_live(&claimed) {
                Some(claimed)
            } else {
                self.ownership.release_pty(&claimed);
                None
            }
        });
        if launch_owned && claimed.is_none() {
            self.ownership
                .launch_claims
                .lock()
                .insert(self.tab_id.clone(), candidate_pty_id.clone());
            claimed = Some(candidate_pty_id.clone());
        }
        let selected = claimed.unwrap_or_else(|| candidate_pty_id.clone());
        let retained_candidate = selected == candidate_pty_id;
        if !retained_candidate {
            kill(&candidate_pty_id);
        }
        let linked = match link(&selected) {
            LinkOutcome::Linked(displaced) => {
                if let Some(displaced) = displaced.filter(|displaced| displaced != &selected) {
                    kill(&displaced);
                }
                true
            }
            LinkOutcome::TabMissing => {
                self.ownership.release_pty(&selected);
                kill(&selected);
                false
            }
        };
        if linked {
            ready(&selected);
        }
        PtySelection {
            pty_id: selected,
            retained_candidate,
            linked,
        }
    }

    #[cfg(test)]
    fn coordinate(
        &self,
        launch_owned: bool,
        mut existing: impl FnMut() -> Option<String>,
        mut spawn: impl FnMut() -> String,
        is_live: impl FnMut(&str) -> bool,
        link: impl FnMut(&str) -> LinkOutcome,
        kill: impl FnMut(&str),
        ready: impl FnMut(&str),
    ) -> PtySelection {
        let candidate = existing().unwrap_or_else(&mut spawn);
        self.settle(candidate, launch_owned, is_live, link, kill, ready)
    }
}

#[cfg(test)]
mod tests;
