use crate::activation::{Activation, ACTIVATION};
use crate::payload::Enforcement;
use crate::policy::Policy;
use crate::verdict::{self, EffectiveAccess};
use crate::verify::{
    self, Accepted, Binding, DetachedGrant, Facets, Refusal, Rejected, VerifiedGrant,
};
use std::sync::{Arc, OnceLock};

/// The single trust decision point; consumers hold it as a trait object so it can move crates.
pub trait EntitlementAuthority: Send + Sync {
    fn verify(
        &self,
        grant: &DetachedGrant,
        binding: &Binding<'_>,
        cached: Option<&VerifiedGrant>,
        now: u64,
    ) -> Result<Accepted, Rejected>;

    /// Authenticates a non-grant signed document (Mesh membership) against the same pinned keys.
    fn authenticate(&self, grant: &DetachedGrant, max_bytes: usize) -> Result<Vec<u8>, Refusal>;

    fn enforcement(&self) -> Enforcement;

    fn compiled_free(&self) -> EffectiveAccess {
        verdict::compiled_free()
    }

    fn activation(&self) -> Activation;

    /// Every refused, missing, expired or not-yet-valid verdict yields compiled free.
    fn access(
        &self,
        grant: Option<&DetachedGrant>,
        binding: &Binding<'_>,
        now: u64,
    ) -> EffectiveAccess {
        grant
            .and_then(|grant| self.verify(grant, binding, None, now).ok())
            .map_or_else(
                || self.compiled_free(),
                |accepted| accepted.verdict.effective(),
            )
    }
}

pub struct PinnedAuthority {
    activation: Activation,
    policy: Option<Policy>,
}

impl PinnedAuthority {
    /// An activation whose policy does not build refuses every grant instead of opening up.
    pub fn new(activation: Activation) -> Self {
        Self {
            activation,
            policy: activation.policy().ok(),
        }
    }
}

impl EntitlementAuthority for PinnedAuthority {
    fn verify(
        &self,
        grant: &DetachedGrant,
        binding: &Binding<'_>,
        cached: Option<&VerifiedGrant>,
        now: u64,
    ) -> Result<Accepted, Rejected> {
        match &self.policy {
            Some(policy) => verify::verify(grant, policy, binding, cached, now),
            None => Err(Rejected {
                refusal: Refusal::UnknownKey,
                facets: Facets::default(),
            }),
        }
    }

    fn authenticate(&self, grant: &DetachedGrant, max_bytes: usize) -> Result<Vec<u8>, Refusal> {
        let policy = self.policy.as_ref().ok_or(Refusal::UnknownKey)?;
        verify::authenticate(grant, policy, max_bytes)
    }

    fn enforcement(&self) -> Enforcement {
        self.activation.enforcement
    }

    fn activation(&self) -> Activation {
        self.activation
    }
}

/// The authority this build pins; there is no runtime input that selects another.
pub fn pinned() -> Arc<dyn EntitlementAuthority> {
    static PINNED: OnceLock<Arc<dyn EntitlementAuthority>> = OnceLock::new();
    PINNED
        .get_or_init(|| Arc::new(PinnedAuthority::new(ACTIVATION)))
        .clone()
}

pub fn enforcement() -> Enforcement {
    ACTIVATION.enforcement
}

pub fn enforcing() -> bool {
    enforcement().enabled
}
