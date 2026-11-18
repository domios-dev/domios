use codeterm_types::config::provider_accounts::ProviderAccountsConfig;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub provider: String,
    pub account: String,
    pub tab: String,
    pub nonce: String,
    pub epoch: u64,
    pub session: String,
    pub authentication_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Authentication {
    SignedOut,
    Ready,
    Exhausted { resets_at: Option<u64> },
}

#[derive(Clone, Debug)]
struct Account {
    authentication: Authentication,
    generation: u64,
    removing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub source: Binding,
    pub target: String,
    pub turn: String,
    generation: u64,
    serial: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Stale,
    AlreadyHandled,
    AutomaticDisabled { next: Option<String> },
    ContinuationUnverified { next: Option<String> },
    AllExhausted,
    Switch(Claim),
}

#[derive(Default)]
pub struct Switches {
    accounts: BTreeMap<(String, String), Account>,
    bindings: BTreeMap<String, Binding>,
    claims: BTreeMap<String, Claim>,
    serial: u64,
    observed: BTreeMap<String, (Binding, String)>,
}

impl Switches {
    pub fn authentication(&self, provider: &str, account: &str) -> Option<Authentication> {
        self.accounts
            .get(&(provider.into(), account.into()))
            .map(|entry| entry.authentication.clone())
    }

    pub fn request(&mut self, binding: &Binding, target: &str, turn: &str) -> Decision {
        if self.bindings.get(&binding.tab) != Some(binding)
            || self.claims.contains_key(&binding.tab)
        {
            return Decision::AlreadyHandled;
        }
        if !self
            .accounts
            .get(&(binding.provider.clone(), binding.account.clone()))
            .is_some_and(|account| {
                account.generation == binding.authentication_generation
                    && account.authentication != Authentication::SignedOut
            })
        {
            return Decision::Stale;
        }
        let Some(account) = self
            .accounts
            .get(&(binding.provider.clone(), target.into()))
            .filter(|account| !account.removing && account.authentication == Authentication::Ready)
        else {
            return Decision::Stale;
        };
        let generation = account.generation;
        self.serial = self.serial.saturating_add(1);
        let claim = Claim {
            source: binding.clone(),
            target: target.into(),
            turn: turn.into(),
            generation,
            serial: self.serial,
        };
        self.claims.insert(binding.tab.clone(), claim.clone());
        Decision::Switch(claim)
    }
    pub fn set_authentication(
        &mut self,
        provider: &str,
        account: &str,
        authentication: Authentication,
    ) {
        let key = (provider.to_owned(), account.to_owned());
        let entry = self.accounts.entry(key).or_insert(Account {
            authentication: Authentication::SignedOut,
            generation: 0,
            removing: false,
        });
        if entry.authentication != authentication {
            if matches!(entry.authentication, Authentication::SignedOut)
                || matches!(authentication, Authentication::SignedOut)
            {
                entry.generation = entry.generation.saturating_add(1);
            }
            entry.authentication = authentication;
        }
    }

    pub fn generation(&self, provider: &str, account: &str) -> Option<u64> {
        self.accounts
            .get(&(provider.to_owned(), account.to_owned()))
            .map(|entry| entry.generation)
    }

    pub fn bind(&mut self, binding: Binding) {
        if self.bindings.get(&binding.tab) == Some(&binding) {
            return;
        }
        self.observed.remove(&binding.tab);
        self.claims.remove(&binding.tab);
        self.bindings.insert(binding.tab.clone(), binding);
    }

    pub fn close_tab(&mut self, tab: &str) {
        self.observed.remove(tab);
        self.claims.remove(tab);
        self.bindings.remove(tab);
    }

    pub fn mark_removing(&mut self, provider: &str, account: &str) {
        if let Some(entry) = self
            .accounts
            .get_mut(&(provider.to_owned(), account.to_owned()))
        {
            entry.removing = true;
        }
    }

    pub fn removable(&self, provider: &str, account: &str) -> bool {
        !self
            .bindings
            .values()
            .any(|binding| binding.provider == provider && binding.account == account)
            && !self.claims.values().any(|claim| {
                claim.source.provider == provider
                    && (claim.source.account == account || claim.target == account)
            })
    }

    pub fn forget(&mut self, provider: &str, account: &str) -> bool {
        if !self.removable(provider, account) {
            return false;
        }
        self.accounts
            .remove(&(provider.to_owned(), account.to_owned()));
        true
    }

    pub fn exhausted(
        &mut self,
        binding: &Binding,
        turn: &str,
        resets_at: Option<u64>,
        policy: &ProviderAccountsConfig,
        continuation_proven: bool,
    ) -> Decision {
        if self.bindings.get(&binding.tab) != Some(binding)
            || binding.session.is_empty()
            || binding.nonce.is_empty()
            || turn.is_empty()
        {
            return Decision::Stale;
        }
        if self.claims.contains_key(&binding.tab)
            || self
                .observed
                .get(&binding.tab)
                .is_some_and(|(seen, seen_turn)| seen == binding && seen_turn == turn)
        {
            return Decision::AlreadyHandled;
        }
        if policy.validate().is_err() || !policy.order.contains(&binding.account) {
            return Decision::Stale;
        }
        if !self
            .accounts
            .get(&(binding.provider.clone(), binding.account.clone()))
            .is_some_and(|entry| {
                entry.generation == binding.authentication_generation
                    && entry.authentication != Authentication::SignedOut
            })
        {
            return Decision::Stale;
        }
        self.observed
            .insert(binding.tab.clone(), (binding.clone(), turn.to_owned()));
        self.set_authentication(
            &binding.provider,
            &binding.account,
            Authentication::Exhausted { resets_at },
        );
        let Ok(next) = policy.next_eligible(&binding.account, |account| {
            self.accounts
                .get(&(binding.provider.clone(), account.to_owned()))
                .is_some_and(|entry| {
                    !entry.removing && entry.authentication == Authentication::Ready
                })
        }) else {
            return Decision::Stale;
        };
        let next = next.map(str::to_owned);
        if next.is_none() {
            return Decision::AllExhausted;
        }
        if !continuation_proven {
            return Decision::ContinuationUnverified { next };
        }
        if !policy.automatic_switch {
            return Decision::AutomaticDisabled { next };
        }
        let Some(target) = next else {
            return Decision::AllExhausted;
        };
        let generation = self.accounts[&(binding.provider.clone(), target.clone())].generation;
        self.serial = self.serial.saturating_add(1);
        let claim = Claim {
            source: binding.clone(),
            target,
            turn: turn.to_owned(),
            generation,
            serial: self.serial,
        };
        self.claims.insert(binding.tab.clone(), claim.clone());
        Decision::Switch(claim)
    }

    pub(super) fn owns(&self, claim: &Claim) -> bool {
        self.claims.get(&claim.source.tab) == Some(claim)
            && self.bindings.get(&claim.source.tab) == Some(&claim.source)
    }

    pub fn current(&self, claim: &Claim) -> bool {
        self.owns(claim)
            && self
                .accounts
                .get(&(claim.source.provider.clone(), claim.source.account.clone()))
                .is_some_and(|entry| {
                    entry.generation == claim.source.authentication_generation
                        && entry.authentication != Authentication::SignedOut
                })
            && self
                .accounts
                .get(&(claim.source.provider.clone(), claim.target.clone()))
                .is_some_and(|entry| {
                    entry.generation == claim.generation
                        && entry.authentication == Authentication::Ready
                })
    }

    pub fn cancel(&mut self, claim: &Claim) {
        if self.claims.get(&claim.source.tab) == Some(claim) {
            self.claims.remove(&claim.source.tab);
        }
    }

    pub fn finish(&mut self, claim: &Claim, resumed: Binding) -> bool {
        if !self.current(claim)
            || resumed.provider != claim.source.provider
            || resumed.account != claim.target
            || resumed.tab != claim.source.tab
            || resumed.session != claim.source.session
            || resumed.nonce == claim.source.nonce
            || resumed.epoch <= claim.source.epoch
            || resumed.authentication_generation != claim.generation
        {
            return false;
        }
        self.bind(resumed);
        true
    }
}

#[cfg(test)]
mod tests;
