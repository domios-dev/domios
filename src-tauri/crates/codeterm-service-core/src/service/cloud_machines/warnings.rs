use std::{collections::BTreeMap, io, path::Path};

use codeterm_account::account_mesh::cloud_provisioning::WireCloudCreditWarnings;
use codeterm_notification_core::{
    contract::{NotificationAction, NotifyLevel},
    store::{
        DurableBellNotice, Notification, NotificationProvenance, NotificationSeverity,
        NotificationStore,
    },
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::service::{
    notification::{PushEnvelope, PushIntent},
    tab_chat::structured_sink::emit_chat_msgs,
    ServiceContext,
};

use super::CloudLifecycleState;
use crate::agent::chat_types::ChatMsg;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
        let stamp = stamp.filter(|stamp| *stamp > 0 && observed.is_none())?;
        *observed = Some(stamp);
        Some((threshold, stamp))
    })
    .collect()
}

#[derive(Default, Serialize, Deserialize)]
struct WarningLedger {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pending: BTreeMap<String, PendingWarning>,
    #[serde(flatten)]
    observed: BTreeMap<String, WireCloudCreditWarnings>,
}

#[derive(Serialize, Deserialize)]
struct PendingWarning {
    threshold: Threshold,
    body: String,
}

static LEDGER_LOCK: Mutex<()> = Mutex::new(());

impl WarningLedger {
    fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        codeterm_util::private_file::write_atomic(path, &serde_json::to_vec(self)?)
    }

    fn stage(&mut self, account: &str, warnings: WireCloudCreditWarnings, body: &str) -> bool {
        let previous = self.observed.entry(account.to_owned()).or_default();
        let before = *previous;
        for (threshold, _) in newly_observed(previous, warnings) {
            self.pending.insert(
                threshold.notice_id(account, warnings.epoch),
                PendingWarning {
                    threshold,
                    body: body.to_owned(),
                },
            );
        }
        before != *previous
    }
}

impl Threshold {
    fn notice_id(self, account: &str, epoch: u64) -> String {
        let threshold = match self {
            Self::Percent80 => 80,
            Self::Percent95 => 95,
            Self::Exhausted => 100,
        };
        let digest = Sha256::digest(format!("{account}:{epoch}:{threshold}").as_bytes());
        format!("cloud-hours-{digest:x}")
    }

    fn presentation(self) -> (&'static str, NotificationSeverity, Option<NotifyLevel>) {
        use NotificationSeverity::{Info, Warning};
        match self {
            Self::Percent80 => ("80% of cloud hours used", Info, None),
            Self::Percent95 => (
                "95% of cloud hours used",
                Warning,
                Some(NotifyLevel::Verbose),
            ),
            Self::Exhausted => (
                "Out of cloud hours",
                Warning,
                Some(NotifyLevel::ActionNeeded),
            ),
        }
    }
}

impl PendingWarning {
    fn bell_notice(&self, id: &str) -> DurableBellNotice {
        let (title, severity, level) = self.threshold.presentation();
        DurableBellNotice {
            id: id.into(),
            title: title.into(),
            body: self.body.clone(),
            severity,
            level,
            actions: vec![
                NotificationAction::Url {
                    label: "Top up".into(),
                    value: "domios://cloud/top-up".into(),
                },
                NotificationAction::Url {
                    label: "Open cloud machines".into(),
                    value: "domios://cloud/machines".into(),
                },
            ],
        }
    }
}

fn deliver_pending(
    ledger: &mut WarningLedger,
    bell: &mut NotificationStore,
    mut mark_delivered: impl FnMut(&WarningLedger) -> io::Result<()>,
) -> io::Result<Vec<Notification>> {
    let mut delivered = Vec::new();
    while let Some((id, pending)) = ledger.pending.pop_first() {
        if let Some(notification) = bell.add_durable_bell(pending.bell_notice(&id))? {
            delivered.push(notification);
        }
        mark_delivered(ledger)?;
    }
    Ok(delivered)
}

impl ServiceContext {
    pub fn has_unseen_cloud_hours(&self, account: &str, warnings: WireCloudCreditWarnings) -> bool {
        let mut previous = WarningLedger::load(&self.cloud_hours_warning_path())
            .ok()
            .and_then(|mut ledger| ledger.observed.remove(account))
            .unwrap_or_default();
        !newly_observed(&mut previous, warnings).is_empty()
    }

    fn cloud_hours_warning_path(&self) -> std::path::PathBuf {
        self.agent_pad_data_dir.join("cloud-hour-warnings.json")
    }

    pub fn observe_cloud_hours(
        &self,
        account: &str,
        remaining: Option<f64>,
        warnings: WireCloudCreditWarnings,
        backend_machines: Option<Vec<(String, Option<String>)>>,
    ) {
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
        let balance = remaining.map_or_else(
            || "Cloud hours remaining are temporarily unavailable.".into(),
            |remaining| format!("{:.2} cloud hours left.", remaining.max(0.0)),
        );
        let body = if names.is_empty() {
            balance
        } else {
            format!("{balance} Affected machines: {}.", names.join(", "))
        };
        match self.drain_cloud_hours_warnings(Some((account, warnings, &body))) {
            Ok(notifications) => {
                for notification in notifications {
                    self.publish_cloud_hours_warning(notification);
                }
            }
            Err(error) => log::debug!("Could not deliver cloud hours warnings: {error}"),
        }
    }

    pub(crate) fn replay_cloud_hours_warnings(&self) {
        if let Err(error) = self.drain_cloud_hours_warnings(None) {
            log::debug!("Could not replay pending cloud hours warnings: {error}");
        }
    }

    fn drain_cloud_hours_warnings(
        &self,
        observation: Option<(&str, WireCloudCreditWarnings, &str)>,
    ) -> io::Result<Vec<Notification>> {
        let _guard = LEDGER_LOCK.lock();
        let path = self.cloud_hours_warning_path();
        let mut ledger = WarningLedger::load(&path)?;
        if let Some((account, warnings, body)) = observation {
            if ledger.stage(account, warnings, body) {
                ledger.save(&path)?;
            }
        }
        if ledger.pending.is_empty() {
            return Ok(Vec::new());
        }
        deliver_pending(&mut ledger, &mut self.notif.lock(), |ledger| {
            ledger.save(&path)
        })
    }

    fn publish_cloud_hours_warning(&self, notification: Notification) {
        let surfaces = self.resolve_surface_dispatch(notification.level);
        if crate::notification::desktop::admit(surfaces.declared, &surfaces.desktop)
            .pass()
            .is_some()
        {
            let _ = self
                .shell
                .os_notify(&notification.title, notification.body.as_deref());
        }
        if notification.level.is_some() {
            let plan = self.resolve_push_plan(PushIntent {
                level: notification.level,
                event_key: Some(notification.id.clone()),
            });
            if let Some(level) = plan.intent.level {
                if self
                    .notif
                    .lock()
                    .admit_durable_bell_push("cloud-hours", plan.per_scope_limit)
                {
                    let envelope = PushEnvelope {
                        content: crate::notification::push::PushContent {
                            level,
                            title: notification.title.clone(),
                            body: notification.body.clone(),
                            provenance: NotificationProvenance::default(),
                        },
                        choices: Vec::new(),
                        record: notification.id.clone(),
                    };
                    if let Err(error) = crate::service::notification_push::deliver_plan(
                        &plan.targets,
                        &plan.refusals,
                        self,
                        &envelope,
                    ) {
                        log::debug!("Could not push cloud hours warning: {error}");
                        self.notif.lock().mark_push_failed(&notification.id);
                    }
                }
            }
        }
        self.notify_event(
            "notification-created",
            serde_json::to_value(notification).unwrap_or_default(),
        );
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
