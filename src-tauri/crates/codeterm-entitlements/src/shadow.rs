use crate::payload::Authority;
use crate::verdict::Verdict;
use serde::Deserialize;

/// The schema-2 envelope's `application_access`, read only for comparison.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ApplicationAccess {
    pub schema_version: u64,
    pub authority: Authority,
    pub full_access: bool,
    pub max_mesh_machines: Option<u64>,
    pub max_workspaces: Option<u64>,
    pub priority_support: bool,
    pub sponsor: bool,
    #[serde(default)]
    pub admin_generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divergence {
    Authority {
        grant: Authority,
        envelope: Authority,
    },
    MeshMachines {
        grant: Option<u64>,
        envelope: Option<u64>,
    },
    Workspaces {
        grant: Option<u64>,
        envelope: Option<u64>,
    },
    Sponsor {
        grant: bool,
        envelope: bool,
    },
    PrioritySupport {
        grant: bool,
        envelope: bool,
    },
}

/// Phase A diagnostic: what the signed verdict would grant versus what the envelope grants.
pub fn shadow(verdict: &Verdict, access: &ApplicationAccess) -> Vec<Divergence> {
    let effective = verdict.effective();
    let mut divergences = Vec::new();
    if effective.authority != access.authority {
        divergences.push(Divergence::Authority {
            grant: effective.authority,
            envelope: access.authority,
        });
    }
    let machines = effective.limits.mesh_machines.max;
    if machines != access.max_mesh_machines {
        divergences.push(Divergence::MeshMachines {
            grant: machines,
            envelope: access.max_mesh_machines,
        });
    }
    let workspaces = effective.limits.workspaces.max;
    if workspaces != access.max_workspaces {
        divergences.push(Divergence::Workspaces {
            grant: workspaces,
            envelope: access.max_workspaces,
        });
    }
    if effective.perks.sponsor != access.sponsor {
        divergences.push(Divergence::Sponsor {
            grant: effective.perks.sponsor,
            envelope: access.sponsor,
        });
    }
    if effective.perks.priority_support != access.priority_support {
        divergences.push(Divergence::PrioritySupport {
            grant: effective.perks.priority_support,
            envelope: access.priority_support,
        });
    }
    divergences
}
