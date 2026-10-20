use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_plugin::local_recording::{LocalRecordingRegistry, SelectedDevices};
use parking_lot::Mutex;
use serde::Serialize;

use super::chat::ChatIdentity;
use super::clip::CaptureClips;
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
    pub(crate) plugin_id: String,
    pub(crate) tab_id: String,
    pub(crate) microphone_id: String,
    pub(crate) generation: u64,
}

#[derive(Default)]
pub struct RecordingLeases {
    state: Mutex<LeaseState>,
    write_tickets: Mutex<HashMap<String, WriteTicket>>,
    clips: CaptureClips,
}

struct WriteTicket {
    lease: String,
    plugin_id: String,
    generation: u64,
    capture: Capture,
}

#[derive(Default)]
struct LeaseState {
    captures: HashMap<String, Capture>,
    generations: HashMap<String, u64>,
    manual_capture: bool,
}

impl RecordingLeases {
    pub fn set_manual_capture(&self, active: bool) -> Result<(), String> {
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
        let stale = state
            .captures
            .iter()
            .filter_map(|(lease, capture)| {
                (!capture.is_current(Instant::now())).then_some(lease.clone())
            })
            .collect::<Vec<_>>();
        for lease in stale {
            if let Some(capture) = state.captures.remove(&lease) {
                capture.active.store(false, Ordering::Release);
            }
            state.generations.remove(&lease);
        }
        if !state.captures.is_empty() {
            return Err("parallel_capture".into());
        }
        let active = registry
            .begin(plugin_id, devices, generation)
            .map_err(str::to_string)?;
        let lease = uuid::Uuid::new_v4().to_string();
        let view = CaptureLeaseView {
            lease: lease.clone(),
            plugin_id: plugin_id.into(),
            tab_id: target.tab_id().into(),
            microphone_id: devices.microphone_id.clone(),
            generation,
        };
        state.captures.insert(
            lease.clone(),
            Capture {
                lease_id: lease.clone(),
                plugin_id: plugin_id.into(),
                target,
                active,
                expires_at: Arc::new(Mutex::new(Instant::now() + Duration::from_secs(600))),
                phase: CapturePhase::Recording,
            },
        );
        state.generations.insert(lease.clone(), generation);
        Ok(view)
    }

    pub fn stopped(&self, lease: &str) -> Result<(), String> {
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

    #[cfg(test)]
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

    pub fn finish(&self, lease: &str) {
        let mut state = self.state.lock();
        state.captures.remove(lease);
        state.generations.remove(lease);
        let cleanup = self.clips.retire_lease(lease);
        drop(state);
        schedule_clip_cleanup(cleanup);
    }

    pub(crate) fn reserve_clip(&self, lease: &str) -> Result<String, String> {
        let state = self.state.lock();
        let capture = state.captures.get(lease).ok_or("capture_lease_stale")?;
        capture.validate()?;
        if capture.phase != CapturePhase::Stopped {
            return Err("capture_not_stopped".into());
        }
        let generation = state
            .generations
            .get(lease)
            .copied()
            .ok_or("capture_lease_stale")?;
        Ok(self.clips.reserve(lease, &capture.plugin_id, generation))
    }

    pub(crate) fn finish_clip_upload(
        &self,
        lease: &str,
        handle: &str,
        path: std::path::PathBuf,
    ) -> Result<(), String> {
        if let Err(path) = self.clips.finish_upload(handle, path) {
            schedule_clip_cleanup(vec![path]);
            return Err("capture_lease_stale".into());
        }
        let state = self.state.lock();
        let generation = state.generations.get(lease).copied();
        let current = state.captures.get(lease).is_some_and(|capture| {
            capture.phase == CapturePhase::Stopped
                && capture.validate().is_ok()
                && generation.is_some_and(|generation| {
                    self.clips
                        .matches_generation(handle, lease, &capture.plugin_id, generation)
                })
        });
        drop(state);
        if !current {
            let cleanup = self.clips.discard(lease, handle).into_iter().collect();
            schedule_clip_cleanup(cleanup);
            return Err("capture_lease_stale".into());
        }
        Ok(())
    }

    pub(crate) fn abandon_clip_upload(&self, handle: &str) {
        self.clips.abandon_upload(handle);
    }

    pub(crate) fn transcribing_clip(
        &self,
        lease: &str,
        handle: &str,
    ) -> Result<(Capture, std::path::PathBuf), String> {
        let mut state = self.state.lock();
        let generation = state
            .generations
            .get(lease)
            .copied()
            .ok_or("capture_lease_stale")?;
        let capture = state.captures.get_mut(lease).ok_or("capture_lease_stale")?;
        capture.validate()?;
        if capture.phase != CapturePhase::Stopped {
            return Err("capture_not_stopped".into());
        }
        if !self
            .clips
            .matches_generation(handle, lease, &capture.plugin_id, generation)
        {
            return Err("capture_clip_stale".into());
        }
        let path = self.clips.begin_transcription(lease, handle)?;
        capture.phase = CapturePhase::Transcribing;
        Ok((capture.clone(), path))
    }

    pub(crate) fn mark_clip_handoff_possible(
        &self,
        lease: &str,
        handle: &str,
    ) -> Result<(), String> {
        let state = self.state.lock();
        let generation = state
            .generations
            .get(lease)
            .copied()
            .ok_or("capture_lease_stale")?;
        let capture = state.captures.get(lease).ok_or("capture_lease_stale")?;
        capture.validate()?;
        if capture.phase != CapturePhase::Transcribing {
            return Err("capture_not_transcribing".into());
        }
        if !self
            .clips
            .matches_generation(handle, lease, &capture.plugin_id, generation)
        {
            return Err("capture_clip_stale".into());
        }
        self.clips.mark_handoff_possible(lease, handle)
    }

    pub(crate) fn finish_clip_pre_handoff(&self, lease: &str, handle: &str) {
        schedule_clip_cleanup(
            self.clips
                .finish_pre_handoff(lease, handle)
                .into_iter()
                .collect(),
        );
    }

    pub(crate) fn retain_clip_after_handoff(
        &self,
        lease: &str,
        handle: &str,
    ) -> Result<(), String> {
        self.clips.retain_after_handoff(lease, handle)
    }

    pub(crate) fn discard_clip(&self, lease: &str, handle: &str) {
        schedule_clip_cleanup(self.clips.discard(lease, handle).into_iter().collect());
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
        let generation = self
            .state
            .lock()
            .generations
            .get(lease)
            .copied()
            .ok_or_else(|| "capture_lease_stale".to_string())?;
        capture.validate()?;
        let ticket_capture = capture.clone();
        let mut tickets = self.write_tickets.lock();
        if tickets.contains_key(send_id) || tickets.len() >= MAX_CAPTURE_WRITE_TICKETS {
            return Err("recording_write_ticket_capacity".into());
        }
        tickets.insert(
            send_id.into(),
            WriteTicket {
                lease: lease.into(),
                plugin_id: capture.plugin_id.clone(),
                generation,
                capture,
            },
        );
        drop(tickets);
        if let Err(error) = ticket_capture.validate() {
            self.write_tickets.lock().remove(send_id);
            return Err(error);
        }
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
        let mut state = self.state.lock();
        state.generations.remove(lease);
        if let Some(capture) = state.captures.remove(lease) {
            capture.active.store(false, Ordering::Release);
        }
        let cleanup = self.clips.retire_lease(lease);
        drop(state);
        schedule_clip_cleanup(cleanup);
    }

    pub(crate) fn invalidate_generation(&self, plugin_id: &str, generation: u64) -> usize {
        let mut invalidated = HashSet::new();
        {
            let mut state = self.state.lock();
            let leases = state
                .captures
                .iter()
                .filter_map(|(lease, capture)| {
                    let matches = capture.plugin_id == plugin_id
                        && state.generations.get(lease) == Some(&generation);
                    matches.then_some(lease.clone())
                })
                .collect::<Vec<_>>();
            for lease in leases {
                if let Some(capture) = state.captures.remove(&lease) {
                    capture.active.store(false, Ordering::Release);
                    invalidated.insert(lease.clone());
                }
                state.generations.remove(&lease);
            }
        }
        {
            let mut tickets = self.write_tickets.lock();
            tickets.retain(|_, ticket| {
                let matches = ticket.plugin_id == plugin_id && ticket.generation == generation;
                if matches {
                    ticket.capture.active.store(false, Ordering::Release);
                    invalidated.insert(ticket.lease.clone());
                }
                !matches
            });
        }
        let cleanup = self.clips.invalidate_generation(plugin_id, generation);
        schedule_clip_cleanup(cleanup);
        invalidated.len()
    }

    pub fn close_tab(&self, tab_id: &str) {
        let mut state = self.state.lock();
        let leases = state
            .captures
            .iter()
            .filter_map(|(lease, capture)| {
                (capture.target.tab_id() == tab_id).then_some(lease.clone())
            })
            .collect::<Vec<_>>();
        let mut cleanup = Vec::new();
        for lease in leases {
            if let Some(capture) = state.captures.remove(&lease) {
                capture.active.store(false, Ordering::Release);
            }
            state.generations.remove(&lease);
            cleanup.extend(self.clips.retire_lease(&lease));
        }
        drop(state);
        schedule_clip_cleanup(cleanup);
    }
}

fn schedule_clip_cleanup(paths: Vec<std::path::PathBuf>) {
    if !paths.is_empty() {
        crate::async_runtime::spawn_blocking(move || CaptureClips::cleanup(paths));
    }
}

impl Capture {
    pub(crate) fn is_current(&self, now: Instant) -> bool {
        self.active.load(Ordering::Acquire) && now < *self.expires_at.lock()
    }

    pub fn validate(&self) -> Result<(), String> {
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
