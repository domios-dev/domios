use super::types::{CloudError, CloudLifecycleState, CloudMachineRecord};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloudTarget {
    Auto,
    New,
    Machine(String),
}

impl CloudTarget {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
            Self::Auto
        } else if trimmed.eq_ignore_ascii_case("new") {
            Self::New
        } else {
            Self::Machine(trimmed.to_owned())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementDecision {
    ReuseAwake(String),
    WakeAndReuse(String),
    CreateExplicit,
}

pub fn select_placement(
    machines: &[CloudMachineRecord],
    target: &CloudTarget,
) -> Result<PlacementDecision, CloudError> {
    match target {
        CloudTarget::New => Ok(PlacementDecision::CreateExplicit),
        CloudTarget::Auto => {
            let mut awake: Vec<&CloudMachineRecord> = machines
                .iter()
                .filter(|m| m.state == CloudLifecycleState::Awake)
                .collect();
            awake.sort_by(|a, b| {
                b.last_awake_at_ms
                    .cmp(&a.last_awake_at_ms)
                    .then_with(|| a.machine_id.cmp(&b.machine_id))
            });
            if let Some(best) = awake.first() {
                return Ok(PlacementDecision::ReuseAwake(best.machine_id.clone()));
            }

            let mut wakeable: Vec<&CloudMachineRecord> = machines
                .iter()
                .filter(|m| {
                    matches!(
                        m.state,
                        CloudLifecycleState::Waking
                            | CloudLifecycleState::Asleep
                            | CloudLifecycleState::Draining
                    )
                })
                .collect();
            wakeable.sort_by(|a, b| {
                let rank = |s: CloudLifecycleState| match s {
                    CloudLifecycleState::Waking => 0u8,
                    CloudLifecycleState::Asleep => 1u8,
                    CloudLifecycleState::Draining => 2u8,
                    _ => 3u8,
                };
                rank(a.state)
                    .cmp(&rank(b.state))
                    .then_with(|| b.created_at_ms.cmp(&a.created_at_ms))
                    .then_with(|| a.machine_id.cmp(&b.machine_id))
            });
            if let Some(best) = wakeable.first() {
                return Ok(PlacementDecision::WakeAndReuse(best.machine_id.clone()));
            }

            if let Some(exhausted) = machines
                .iter()
                .find(|m| m.state == CloudLifecycleState::OutOfCredits)
            {
                return Err(CloudError::MachineOutOfCredits {
                    machine_id: exhausted.machine_id.clone(),
                });
            }

            // Never create implicitly when --cloud auto finds no reusable machine.
            Err(CloudError::ImplicitCreateRefused)
        }
        CloudTarget::Machine(selector) => {
            let matches: Vec<&CloudMachineRecord> = machines
                .iter()
                .filter(|m| m.state != CloudLifecycleState::Deleted && m.matches_selector(selector))
                .collect();
            match matches.as_slice() {
                [] => Err(CloudError::MachineNotFound(selector.clone())),
                [single] => match single.state {
                    CloudLifecycleState::Awake => {
                        Ok(PlacementDecision::ReuseAwake(single.machine_id.clone()))
                    }
                    CloudLifecycleState::Asleep
                    | CloudLifecycleState::Draining
                    | CloudLifecycleState::Waking => {
                        Ok(PlacementDecision::WakeAndReuse(single.machine_id.clone()))
                    }
                    CloudLifecycleState::OutOfCredits => Err(CloudError::MachineOutOfCredits {
                        machine_id: single.machine_id.clone(),
                    }),
                    state => Err(CloudError::InvalidTransition {
                        from: state,
                        event: "placement_use",
                    }),
                },
                _ => Err(CloudError::MachineAmbiguous(selector.clone())),
            }
        }
    }
}
