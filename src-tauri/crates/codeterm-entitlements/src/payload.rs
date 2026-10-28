use crate::text::{safe_integer, scalar_text, seconds, valid_id, valid_installation_id};
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;

pub const ISSUER: &str = "domios-entitlements";
pub const MAX_PAYLOAD_BYTES: usize = 8192;
pub const MAX_CAPABILITIES: usize = 64;
pub const REFRESH_SECS: u64 = 3_600;
pub const LEASE_SECS: u64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Test,
    Live,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Admin,
    Billing,
    Free,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Label {
    Admin,
    Pro,
    Sponsor,
    Free,
    Paid(String),
}

impl Label {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Admin => "admin",
            Self::Pro => "pro",
            Self::Sponsor => "sponsor",
            Self::Free => "free",
            Self::Paid(name) => name,
        }
    }
}

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if !scalar_text(&name, 128) {
            return Err(serde::de::Error::custom("Invalid tier label"));
        }
        Ok(match name.as_str() {
            "admin" => Self::Admin,
            "pro" => Self::Pro,
            "sponsor" => Self::Sponsor,
            "free" => Self::Free,
            _ => Self::Paid(name),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Account,
    Installation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionReason {
    PeriodEnded,
    Cancelled,
    Downgraded,
    Revoked,
    GiftExpired,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audience {
    pub project: String,
    pub env: Mode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enforcement {
    pub enabled: bool,
    pub rollback: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedLimit {
    #[serde(deserialize_with = "nullable")]
    pub max: Option<u64>,
    pub scope: Scope,
}

/// Limits as signed. Policies older than `TAB_TEAM_LIMITS_POLICY_VERSION` sign only the first three keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLimits {
    #[serde(rename = "mesh.machines")]
    pub mesh_machines: ScopedLimit,
    pub workspaces: ScopedLimit,
    #[serde(rename = "mem.items")]
    pub mem_items: ScopedLimit,
    #[serde(default, deserialize_with = "present_limit")]
    pub teams: Option<ScopedLimit>,
    #[serde(default, rename = "agent.tabs", deserialize_with = "present_limit")]
    pub agent_tabs: Option<ScopedLimit>,
}

/// The first producer policy version that signs `teams` and `agent.tabs`.
pub const TAB_TEAM_LIMITS_POLICY_VERSION: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub mesh_machines: ScopedLimit,
    pub workspaces: ScopedLimit,
    pub mem_items: ScopedLimit,
    pub teams: ScopedLimit,
    pub agent_tabs: ScopedLimit,
}

pub const COMPILED_FREE_LIMITS: Limits = Limits {
    mesh_machines: ScopedLimit {
        max: Some(2),
        scope: Scope::Account,
    },
    workspaces: ScopedLimit {
        max: Some(2),
        scope: Scope::Installation,
    },
    mem_items: ScopedLimit {
        max: Some(500),
        scope: Scope::Installation,
    },
    teams: ScopedLimit {
        max: Some(6),
        scope: Scope::Installation,
    },
    agent_tabs: ScopedLimit {
        max: Some(35),
        scope: Scope::Installation,
    },
};

impl SignedLimits {
    /// An unsigned key takes the producer catalog row for the signed authority: free is the
    /// compiled baseline, paid and admin are unlimited.
    pub fn resolve(&self, authority: Authority) -> Limits {
        let catalog = |compiled: ScopedLimit| match authority {
            Authority::Free => compiled,
            Authority::Billing | Authority::Admin => ScopedLimit {
                max: None,
                scope: compiled.scope,
            },
        };
        Limits {
            mesh_machines: self.mesh_machines,
            workspaces: self.workspaces,
            mem_items: self.mem_items,
            teams: self
                .teams
                .unwrap_or_else(|| catalog(COMPILED_FREE_LIMITS.teams)),
            agent_tabs: self
                .agent_tabs
                .unwrap_or_else(|| catalog(COMPILED_FREE_LIMITS.agent_tabs)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Perks {
    pub sponsor: bool,
    pub priority_support: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantPayload {
    pub schema_version: u64,
    pub iss: String,
    pub aud: Audience,
    pub sub: String,
    pub inst: String,
    pub revision: u64,
    pub policy_version: u64,
    pub policy_epoch: String,
    pub issued_at: u64,
    pub not_before: u64,
    pub refresh_after: u64,
    pub expires_at: u64,
    pub grace_until: u64,
    pub authority: Authority,
    pub label: Label,
    pub enforcement: Enforcement,
    pub limits: SignedLimits,
    pub capabilities: Vec<String>,
    pub perks: Perks,
    #[serde(deserialize_with = "nullable")]
    pub period_end: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    pub transition_reason: Option<TransitionReason>,
}

// A `deserialize_with` Option keeps the key required: absent is refused, `null` is None.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

fn present_limit<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ScopedLimit>, D::Error> {
    ScopedLimit::deserialize(d).map(Some)
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<TransitionReason>, D::Error> {
    TransitionReason::deserialize(d).map(Some)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadDefect {
    SchemaVersion,
    Issuer,
    Subject,
    Installation,
    Revision,
    PolicyVersion,
    PolicyEpoch,
    Time,
    PeriodEnd,
    Label,
    Expiry,
    Grace,
    LimitScope,
    LimitMax,
    Capabilities,
    FreeGrant,
}

impl GrantPayload {
    pub fn effective_limits(&self) -> Limits {
        self.limits.resolve(self.authority)
    }

    /// Producer `validatePayload` invariants that do not depend on the policy or on `now`.
    pub fn check_invariants(&self) -> Result<(), PayloadDefect> {
        use PayloadDefect as D;
        ensure(self.schema_version == 1, D::SchemaVersion)?;
        ensure(self.iss == ISSUER, D::Issuer)?;
        ensure(
            valid_id(&self.sub) && scalar_text(&self.sub, 128),
            D::Subject,
        )?;
        ensure(valid_installation_id(&self.inst), D::Installation)?;
        ensure(
            safe_integer(self.revision) && self.revision >= 1,
            D::Revision,
        )?;
        ensure(
            safe_integer(self.policy_version) && self.policy_version >= 1,
            D::PolicyVersion,
        )?;
        ensure(scalar_text(&self.policy_epoch, 128), D::PolicyEpoch)?;
        if self.policy_version >= TAB_TEAM_LIMITS_POLICY_VERSION {
            ensure(
                self.limits.teams.is_some() && self.limits.agent_tabs.is_some(),
                D::LimitMax,
            )?;
        }
        let times = [
            self.issued_at,
            self.not_before,
            self.refresh_after,
            self.expires_at,
            self.grace_until,
        ];
        ensure(times.into_iter().all(seconds), D::Time)?;
        ensure(
            self.not_before == self.issued_at
                && self.refresh_after == self.issued_at + REFRESH_SECS
                && self.issued_at <= self.expires_at
                && self.expires_at <= self.grace_until,
            D::Time,
        )?;
        if let Some(end) = self.period_end {
            ensure(seconds(end) && end > self.issued_at, D::PeriodEnd)?;
        }
        let label_ok = match self.authority {
            Authority::Billing => match self.label {
                Label::Pro | Label::Sponsor => true,
                Label::Paid(_) => self.policy_version >= TAB_TEAM_LIMITS_POLICY_VERSION,
                Label::Admin | Label::Free => false,
            },
            Authority::Admin => self.label == Label::Admin,
            Authority::Free => self.label == Label::Free,
        };
        ensure(label_ok, D::Label)?;
        ensure(
            self.authority == Authority::Billing || self.period_end.is_none(),
            D::PeriodEnd,
        )?;
        ensure(self.expires_at == self.issued_at + LEASE_SECS, D::Expiry)?;
        ensure(self.grace_until == self.expires_at, D::Grace)?;
        let limits = self.effective_limits();
        ensure(
            limits.mesh_machines.scope == Scope::Account
                && limits.workspaces.scope == Scope::Installation
                && limits.mem_items.scope == Scope::Installation
                && limits.teams.scope == Scope::Installation
                && limits.agent_tabs.scope == Scope::Installation,
            D::LimitScope,
        )?;
        let maxima = [
            limits.mesh_machines.max,
            limits.workspaces.max,
            limits.mem_items.max,
            limits.teams.max,
            limits.agent_tabs.max,
        ];
        ensure(maxima.into_iter().flatten().all(safe_integer), D::LimitMax)?;
        let unique: HashSet<&str> = self.capabilities.iter().map(String::as_str).collect();
        ensure(
            self.capabilities.len() <= MAX_CAPABILITIES
                && unique.len() == self.capabilities.len()
                && self.capabilities.iter().all(|name| scalar_text(name, 128)),
            D::Capabilities,
        )?;
        if self.authority == Authority::Free {
            let mut baseline = COMPILED_FREE_LIMITS;
            if self.policy_version < TAB_TEAM_LIMITS_POLICY_VERSION {
                baseline.mem_items.max = Some(1000);
            }
            ensure(
                !self.enforcement.rollback
                    && self.perks == Perks::default()
                    && self.capabilities.is_empty()
                    && limits == baseline,
                D::FreeGrant,
            )?;
        }
        Ok(())
    }
}

fn ensure(condition: bool, defect: PayloadDefect) -> Result<(), PayloadDefect> {
    if condition {
        Ok(())
    } else {
        Err(defect)
    }
}
