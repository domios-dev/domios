//! Account Mesh admission: installation possession key, server proofs, membership trust and
//! the sign-in / sign-out lifecycle that gates every Mesh edge of this installation.

mod admission;
pub mod callable;
mod key;
mod lifecycle;
mod membership;
mod notice;
mod operations;
mod ports;
mod registry_keys;
mod registry_presence;
mod registry_read;
mod rejoin;
mod retry;
mod state;
mod store;
mod sync;
pub mod wire;

pub use key::{verify_possession, InstallationKey, ProofContext};
pub use lifecycle::{AccountMesh, AccountMeshConfig, AccountMeshPorts};
pub use membership::{
    account_id, membership_token, parse_membership_token, verify_link_permit, verify_membership,
    LinkPermitExpectation, MembershipExpectation, MembershipRefusal, MembershipTrust,
    VerifiedMembership, NOT_BEFORE_SKEW_SECONDS,
};
pub use operations::{LinkAuthorization, LinkTarget};
pub use ports::{
    AccountMeshStore, Clock, EndpointSource, InstallationKeyStore, MeshRegistry, MeshTransport,
    RegistryResult, SessionSource, SystemClock, UnavailableRegistry,
};
pub use state::{
    AccountMeshRecord, AccountMeshRefusal, AccountMeshView, Binding, Cleanup, EnrollTrigger,
    Enrollment, LifecycleState, LimitNotice, Presence, Publication, RegistryObservation,
    SelfInstallation, SignedMembership, Traffic,
};
pub use store::FileAccountMeshStore;

use crate::{Identity, IdentityObserver};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
impl IdentityObserver for AccountMesh {
    async fn changed(&self, identity: Option<&Identity>) {
        self.identity_changed(identity).await;
    }
}

/// Fans one identity change out to every observer, in order.
pub struct IdentityObservers(pub Vec<Arc<dyn IdentityObserver>>);

#[async_trait]
impl IdentityObserver for IdentityObservers {
    async fn session_refreshed(&self, session: &crate::Session) {
        for observer in &self.0 {
            observer.session_refreshed(session).await;
        }
    }

    async fn changed(&self, identity: Option<&Identity>) {
        for observer in &self.0 {
            observer.changed(identity).await;
        }
    }
}

#[cfg(test)]
mod tests;
