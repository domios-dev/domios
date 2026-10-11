use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_plugin::local_recording::{LocalRecordingRegistry, SelectedDevices};
use parking_lot::Mutex;
use serde::Serialize;

use super::chat::ChatIdentity;
use crate::service::pending_delivery_payloads::user_sends::UserSendAuthority;

const MAX_CAPTURE_WRITE_TICKETS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FrozenTarget {
    Pty(UserSendAuthority),
    Chat(ChatIdentity),
}

impl FrozenTarget {
    pub(crate) fn tab_id(&self) -> &str {
        match self {
            Self::Pty(authority) => &authority.tab_id,
            Self::Chat(identity) => &identity.tab_id,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Capture {
    pub(crate) lease_id: String,
    pub(crate) plugin_id: String,
    pub(crate) target: FrozenTarget,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) expires_at: Arc<Mutex<Instant>>,
    pub(crate) phase: CapturePhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapturePhase {
    Recording,
    Stopping,
    Stopped,
    Transcribing,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureLeaseView {
    pub(crate) lease: String,
    pub(crate) tab_id: String,
    pub(crate) microphone_id: String,
    pub(crate) generation: u64,
}

#[derive(Default)]
pub(crate) struct RecordingLeases {
    state: Mutex<LeaseState>,
    write_tickets: Mutex<HashMap<String, WriteTicket>>,
}

struct WriteTicket {
    lease: String,
    capture: Capture,
}

#[derive(Default)]
struct LeaseState {
    captures: HashMap<String, Capture>,
    manual_capture: bool,
}

impl RecordingLeases {
    pub(crate) fn set_manual_capture(&self, active: bool) -> Result<(), String> {
        let mut state = self.state.lock();
        if active
            && state
                .captures
                .values()
                .any(|capture| capture.is_current(Instant::now()))
        {
            return Err("parallel_capture".into());
        }
        state.manual_capture = active;
        Ok(())
    }

    pub(crate) fn start(
        &self,
        plugin_id: &str,
        devices: &SelectedDevices,
        generation: u64,
        target: FrozenTarget,
        registry: &LocalRecordingRegistry,
    ) -> Result<CaptureLeaseView, String> {
        let mut state = self.state.lock();
        if state.manual_capture {
            return Err("manual_capture_owned".into());
        }
        state.captures.retain(|_, capture| {
            let current = capture.is_current(Instant::now());
            if !current {
                capture.active.store(false, Ordering::Release);
            }
            current
        });
        if !state.captures.is_empty() {
            return Err("parallel_capture".into());
        }
        let active = registry
            .begin(plugin_id, devices, generation)
            .map_err(str::to_string)?;
        let lease = uuid::Uuid::new_v4().to_string();
        let view = CaptureLeaseView {
            lease: lease.clone(),
            tab_id: target.tab_id().into(),
            microphone_id: devices.microphone_id.clone(),
            generation,
        };
        state.captures.insert(
            lease.clone(),
            Capture {
                lease_id: lease,
                plugin_id: plugin_id.into(),
                target,
                active,
                expires_at: Arc::new(Mutex::new(Instant::now() + Duration::from_secs(600))),
                phase: CapturePhase::Recording,
            },
        );
        Ok(view)
    }

    pub(crate) fn stopped(&self, lease: &str) -> Result<(), String> {
        let mut state = self.state.lock();
        let capture = state.captures.get_mut(lease).ok_or("capture_lease_stale")?;
        capture.validate()?;
        if !matches!(
            capture.phase,
            CapturePhase::Recording | CapturePhase::Stopping
        ) {
            return Err("capture_not_recording".into());
        }
        capture.phase = CapturePhase::Stopped;
        Ok(())
    }

    pub(crate) fn transcribing(&self, lease: &str) -> Result<Capture, String> {
        let mut state = self.state.lock();
        let capture = state.captures.get_mut(lease).ok_or("capture_lease_stale")?;
        capture.validate()?;
        if capture.phase != CapturePhase::Stopped {
            return Err("capture_not_stopped".into());
        }
        capture.phase = CapturePhase::Transcribing;
        Ok(capture.clone())
    }

    pub(crate) fn finish(&self, lease: &str) {
        self.state.lock().captures.remove(lease);
    }

    pub(crate) fn register_write_ticket(
        &self,
        send_id: &str,
        lease: &str,
        capture: Capture,
    ) -> Result<(), String> {
        if capture.lease_id != lease {
            return Err("recording_write_ticket_lease_mismatch".into());
        }
        capture.validate()?;
        let mut tickets = self.write_tickets.lock();
        if tickets.contains_key(send_id) || tickets.len() >= MAX_CAPTURE_WRITE_TICKETS {
            return Err("recording_write_ticket_capacity".into());
        }
        tickets.insert(
            send_id.into(),
            WriteTicket {
                lease: lease.into(),
                capture,
            },
        );
        Ok(())
    }

    pub(crate) fn has_write_ticket(&self, send_id: &str) -> bool {
        self.write_tickets.lock().contains_key(send_id)
    }

    pub(crate) fn write_ticket_matches_authority(
        &self,
        send_id: &str,
        authority: &UserSendAuthority,
    ) -> bool {
        self.write_tickets
            .lock()
            .get(send_id)
            .is_some_and(|ticket| {
                ticket.capture.validate().is_ok()
                    && matches!(
                        &ticket.capture.target,
                        FrozenTarget::Pty(expected) if expected == authority
                    )
            })
    }

    pub(crate) fn retire_write_ticket(&self, send_id: &str) {
        let ticket = self.write_tickets.lock().remove(send_id);
        if let Some(ticket) = ticket {
            ticket.capture.active.store(false, Ordering::Release);
            self.finish(&ticket.lease);
        }
    }

    #[cfg(test)]
    pub(crate) fn expire_write_ticket(&self, send_id: &str) {
        if let Some(ticket) = self.write_tickets.lock().get_mut(send_id) {
            *ticket.capture.expires_at.lock() = Instant::now() - Duration::from_secs(1);
        }
    }

    pub(crate) fn request_stop(&self, plugin_id: &str) -> Option<Result<String, ()>> {
        let mut state = self.state.lock();
        let (lease, capture) = state.captures.iter_mut().find(|(_, capture)| {
            capture.plugin_id == plugin_id && capture.is_current(Instant::now())
        })?;
        if capture.phase != CapturePhase::Recording {
            return Some(Err(()));
        }
        capture.phase = CapturePhase::Stopping;
        Some(Ok(lease.clone()))
    }

    pub(crate) fn cancel(&self, lease: &str) {
        if let Some(capture) = self.state.lock().captures.remove(lease) {
            capture.active.store(false, Ordering::Release);
        }
    }

    pub(crate) fn close_tab(&self, tab_id: &str) {
        self.state.lock().captures.retain(|_, capture| {
            if capture.target.tab_id() == tab_id {
                capture.active.store(false, Ordering::Release);
                false
            } else {
                true
            }
        });
    }
}

impl Capture {
    pub(crate) fn is_current(&self, now: Instant) -> bool {
        self.active.load(Ordering::Acquire) && now < *self.expires_at.lock()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.is_current(Instant::now()) {
            Ok(())
        } else {
            Err("capture_lease_stale".into())
        }
    }
}

pub(crate) fn resolve_target(
    explicit: Option<String>,
    general: impl FnOnce() -> Option<String>,
) -> Result<String, String> {
    explicit
        .or_else(general)
        .ok_or_else(|| "recording_recipient_unavailable".into())
}

#[cfg(test)]
mod tests;
