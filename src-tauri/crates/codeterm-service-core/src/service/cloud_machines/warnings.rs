use std::{collections::BTreeMap, io, path::Path};

use codeterm_account::account_mesh::cloud_provisioning::WireCloudCreditWarnings;
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};
use parking_lot::Mutex;

use crate::service::{
    notification::{NotificationOrigin, PushIntent},
    tab_chat::structured_sink::emit_chat_msgs,
    ServiceContext,
};

use super::CloudLifecycleState;
use crate::{agent::chat_types::ChatMsg, notification::store::Ask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Threshold {
    Percent80,
    Percent95,
    Exhausted,
}

fn newly_observed(
    previous: &mut WireCloudCreditWarnings,
    incoming: WireCloudCreditWarnings,
) -> Vec<(Threshold, u64)> {
    if incoming.epoch < previous.epoch {
        return Vec::new();
    }
    if incoming.epoch > previous.epoch {
        *previous = WireCloudCreditWarnings {
            epoch: incoming.epoch,
            ..Default::default()
        };
    }
    [
        (
            Threshold::Percent80,
            incoming.pct80_at,
            &mut previous.pct80_at,
        ),
        (
            Threshold::Percent95,
            incoming.pct95_at,
            &mut previous.pct95_at,
        ),
        (
            Threshold::Exhausted,
            incoming.exhausted_at,
            &mut previous.exhausted_at,
        ),
    ]
    .into_iter()
    .filter_map(|(threshold, stamp, observed)| {
        let stamp = stamp.filter(|stamp| *stamp > observed.unwrap_or(0))?;
        *observed = Some(stamp);
        Some((threshold, stamp))
    })
    .collect()
}

fn claim_warnings(
    path: &Path,
    account: &str,
    warnings: WireCloudCreditWarnings,
) -> io::Result<Vec<(Threshold, u64)>> {
    static CLAIM_LOCK: Mutex<()> = Mutex::new(());
    let _guard = CLAIM_LOCK.lock();
    let mut accounts: BTreeMap<String, WireCloudCreditWarnings> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error),
    };
    let previous = accounts.entry(account.to_owned()).or_default();
    let before = *previous;
    let crossed = newly_observed(previous, warnings);
    if *previous != before {
        codeterm_util::private_file::write_atomic(path, &serde_json::to_vec(&accounts)?)?;
    }
    Ok(crossed)
}

impl ServiceContext {
    pub fn observe_cloud_hours(
        &self,
        account: &str,
        remaining: f64,
        warnings: WireCloudCreditWarnings,
        backend_machines: Option<Vec<(String, Option<String>)>>,
    ) {
        let path = codeterm_paths::data_dir().join("cloud-hour-warnings.json");
        let crossed = match claim_warnings(&path, account, warnings) {
            Ok(crossed) => crossed,
            Err(error) => {
                log::debug!("Could not record cloud hours warnings: {error}");
                return;
            }
        };
        if crossed.is_empty() {
            return;
        }

        let records = self.cloud_machines().list_records();
        let names: Vec<_> = match backend_machines {
            Some(ref machines) => machines
                .iter()
                .map(|(id, name)| {
                    records
                        .iter()
                        .find(|record| record.ownership.provider_machine_id == *id)
                        .map(|record| record.name.as_str())
                        .or(name.as_deref())
                        .unwrap_or(id)
                })
                .collect(),
            None => records
                .iter()
                .filter(|machine| {
                    matches!(
                        machine.state,
                        CloudLifecycleState::Awake
                            | CloudLifecycleState::Draining
                            | CloudLifecycleState::OutOfCredits
                    )
                })
                .map(|machine| machine.name.as_str())
                .collect(),
        };
        let remaining = remaining.max(0.0);
        let body = if names.is_empty() {
            format!("{remaining:.2} cloud hours left across your cloud machines.")
        } else {
            format!(
                "{remaining:.2} cloud hours left. Affected machines: {}.",
                names.join(", ")
            )
        };
        for (threshold, stamp) in crossed {
            let (title, severity, level) = match threshold {
                Threshold::Percent80 => ("80% of cloud hours used", "info", None),
                Threshold::Percent95 => (
                    "95% of cloud hours used",
                    "warning",
                    Some(NotifyLevel::Verbose),
                ),
                Threshold::Exhausted => (
                    "Out of cloud hours",
                    "warning",
                    Some(NotifyLevel::ActionNeeded),
                ),
            };
            self.create_notification_actions_reported(
                title.into(),
                Some(body.clone()),
                Some(severity),
                Ask::offering(Some(vec![
                    NotificationAction::Url {
                        label: "Top up".into(),
                        value: "domios://cloud/top-up".into(),
                    },
                    NotificationAction::Url {
                        label: "Open cloud machines".into(),
                        value: "domios://cloud/machines".into(),
                    },
                ])),
                Some(true),
                NotificationOrigin::default(),
                PushIntent {
                    level,
                    event_key: Some(format!(
                        "cloud-hours:{account}:{}:{threshold:?}:{stamp}",
                        warnings.epoch
                    )),
                },
                None,
            );
        }
        self.ui.emit("cloud-hours-changed", serde_json::Value::Null);
    }

    pub fn show_cloud_hours_exhausted_in_tabs(&self, lease_expires_at_ms: u64) {
        let tabs = {
            let mux = self.mux.read();
            mux.all_tabs()
                .filter_map(|tab| {
                    Some((
                        tab.id.clone(),
                        tab.agent_session_id.clone()?,
                        tab.session_binding_nonce.clone(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        for (tab, session, nonce) in tabs {
            if !self.tab_has_active_agent_turn(&tab) {
                continue;
            }
            emit_chat_msgs(
                self.ui.as_ref(),
                &tab,
                &[ChatMsg {
                    id: format!("cloud-hours-exhausted-{lease_expires_at_ms}"),
                    kind: "system".into(),
                    content:
                        "This cloud machine is about to sleep because its cloud hours ran out."
                            .into(),
                    ..Default::default()
                }],
                None,
                Some(&session),
                nonce.as_deref(),
            );
        }
    }
}

#[cfg(test)]
mod tests;
