use crate::{Entitlements, Plan, Status};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Version<const N: u64>;
impl<const N: u64> Serialize for Version<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(N)
    }
}
impl<'de, const N: u64> Deserialize<'de> for Version<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u64::deserialize(deserializer)? {
            value if value == N => Ok(Self),
            value => Err(serde::de::Error::custom(format!(
                "unsupported schema_version {value}, expected {N}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationEnvelope {
    pub schema_version: Version<2>,
    pub billing: Option<Effective>,
    pub billing_inventory: BillingInventory,
    pub application_access: ApplicationAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<serde_json::Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effective {
    pub schema_version: Version<1>,
    pub plan: Plan,
    pub capabilities: Capabilities,
    pub period_end: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub max_mesh_machines: Option<u64>,
    pub max_workspaces: Option<u64>,
    pub sponsor: bool,
    pub priority_support: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingInventory {
    Verified,
    RequiresInventory,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    Admin,
    Billing,
    Free,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationAccess {
    pub schema_version: Version<1>,
    pub authority: Authority,
    pub full_access: bool,
    pub max_mesh_machines: Option<u64>,
    pub max_workspaces: Option<u64>,
    pub priority_support: bool,
    pub sponsor: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_generation: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Access {
    pub authority: Authority,
    pub full_access: bool,
    pub max_mesh_machines: Option<u64>,
    pub max_workspaces: Option<u64>,
    pub sponsor: bool,
    pub priority_support: bool,
}
impl From<&ApplicationAccess> for Access {
    fn from(access: &ApplicationAccess) -> Self {
        Self {
            authority: access.authority,
            full_access: access.full_access,
            max_mesh_machines: access.max_mesh_machines,
            max_workspaces: access.max_workspaces,
            sponsor: access.sponsor,
            priority_support: access.priority_support,
        }
    }
}
impl Capabilities {
    pub fn for_plan(plan: Plan) -> Self {
        match plan {
            Plan::Free => Self {
                max_mesh_machines: Some(2),
                max_workspaces: Some(2),
                sponsor: false,
                priority_support: false,
            },
            Plan::Pro => Self {
                max_mesh_machines: None,
                max_workspaces: None,
                sponsor: false,
                priority_support: false,
            },
            Plan::Sponsor => Self {
                max_mesh_machines: None,
                max_workspaces: None,
                sponsor: true,
                priority_support: true,
            },
        }
    }
}
impl Access {
    /// Mirrors the producer's non-admin projection for adapters that hold plain entitlements.
    pub fn for_entitlements(entitlements: &Entitlements) -> Self {
        let paid = matches!(entitlements.status, Status::Active | Status::Trialing);
        let plan = if paid { entitlements.plan } else { Plan::Free };
        let capabilities = Capabilities::for_plan(plan);
        Self {
            authority: if plan == Plan::Free {
                Authority::Free
            } else {
                Authority::Billing
            },
            full_access: false,
            max_mesh_machines: capabilities.max_mesh_machines,
            max_workspaces: capabilities.max_workspaces,
            sponsor: capabilities.sponsor,
            priority_support: capabilities.priority_support,
        }
    }
}

/// One provider reply: `entitlements` is `None` when the producer still requires an inventory pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub grant: Option<serde_json::Value>,
    pub entitlements: Option<Entitlements>,
    pub access: Access,
}
impl Reading {
    pub fn current(entitlements: Entitlements) -> Self {
        Self {
            grant: None,
            access: Access::for_entitlements(&entitlements),
            entitlements: Some(entitlements),
        }
    }
}
impl From<ApplicationEnvelope> for Reading {
    fn from(envelope: ApplicationEnvelope) -> Self {
        let access = Access::from(&envelope.application_access);
        let entitlements = match envelope.billing_inventory {
            BillingInventory::RequiresInventory => None,
            BillingInventory::Verified => Some(Entitlements {
                plan: envelope.billing.as_ref().map_or(Plan::Free, |b| b.plan),
                status: Status::Active,
                current_period_end: envelope
                    .billing
                    .as_ref()
                    .and_then(|b| b.period_end)
                    .unwrap_or(0),
                cancel_at_period_end: false,
            }),
        };
        Self {
            grant: envelope.grant,
            entitlements,
            access,
        }
    }
}
impl Access {
    pub(crate) fn from_effective(access: &codeterm_entitlements::EffectiveAccess) -> Self {
        Self {
            authority: match access.authority {
                codeterm_entitlements::Authority::Admin => Authority::Admin,
                codeterm_entitlements::Authority::Billing => Authority::Billing,
                codeterm_entitlements::Authority::Free => Authority::Free,
            },
            full_access: access.authority == codeterm_entitlements::Authority::Admin,
            max_mesh_machines: access.limits.mesh_machines.max,
            max_workspaces: access.limits.workspaces.max,
            sponsor: access.perks.sponsor,
            priority_support: access.perks.priority_support,
        }
    }
}
