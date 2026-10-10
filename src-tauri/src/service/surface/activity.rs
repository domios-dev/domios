use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::Emitter;

use crate::api::events::{EventBroadcaster, SystemEvent};
use crate::service::ServiceContext;

pub(crate) const SURFACE_ACTIVITY_EVENT: &str = "surface-activity";
const WINDOW: Duration = Duration::from_millis(250);
const MAX_VERB: usize = 32;
const MAX_ACTOR: usize = 48;
const PRUNE_ABOVE: usize = 256;
const STALE_AFTER: Duration = Duration::from_secs(10);
/// View plumbing, not agent work: these never pulse the owner.
const UI_METHODS: &[&str] = &["resize", "input", "state", "profiles.list"];

/// A bare verb token: anything that could carry an argument or URL is dropped.
pub(crate) fn sanitize_verb(raw: &str) -> String {
    let token = raw.trim();
    let valid = !token.is_empty()
        && token.len() <= MAX_VERB
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        token.to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// A display name on one line, capped; `None` when nothing printable remains.
pub(crate) fn sanitize_actor(raw: &str) -> Option<String> {
    let line: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let name: String = line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_ACTOR)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// One agent verb and, when it is not the owner, who ran it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Activity {
    pub verb: String,
    pub actor: Option<String>,
}

/// Whether a `/call` method is agent work worth showing; only agent callers reach here.
pub(crate) fn is_agent_method(method: &str) -> bool {
    !UI_METHODS.contains(&method) && !method.starts_with("grab.") && !method.starts_with("snip.")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    EmitNow,
    /// Emit the latest verb once `flush` is called after this delay.
    FlushAfter(Duration),
    Coalesced,
}

#[derive(Debug)]
struct Slot {
    last: Instant,
    pending: Option<Activity>,
    flush_armed: bool,
}

/// Per surface tab: one event at once, then at most one trailing event per window.
#[derive(Debug, Default)]
pub(crate) struct SurfaceActivityGate {
    slots: Mutex<HashMap<String, Slot>>,
}

impl SurfaceActivityGate {
    pub(crate) fn offer(&self, tab_id: &str, activity: &Activity, now: Instant) -> Gate {
        let mut slots = self.slots.lock();
        if slots.len() > PRUNE_ABOVE {
            slots.retain(|_, slot| slot.flush_armed || now.duration_since(slot.last) < STALE_AFTER);
        }
        let Some(slot) = slots.get_mut(tab_id) else {
            slots.insert(
                tab_id.to_string(),
                Slot {
                    last: now,
                    pending: None,
                    flush_armed: false,
                },
            );
            return Gate::EmitNow;
        };
        let since = now.duration_since(slot.last);
        if !slot.flush_armed && since >= WINDOW {
            slot.last = now;
            return Gate::EmitNow;
        }
        slot.pending = Some(activity.clone());
        if slot.flush_armed {
            return Gate::Coalesced;
        }
        slot.flush_armed = true;
        Gate::FlushAfter(WINDOW.saturating_sub(since))
    }

    /// The latest activity held back during the window, if any.
    pub(crate) fn flush(&self, tab_id: &str, now: Instant) -> Option<Activity> {
        let mut slots = self.slots.lock();
        let slot = slots.get_mut(tab_id)?;
        slot.flush_armed = false;
        let activity = slot.pending.take()?;
        slot.last = now;
        Some(activity)
    }
}

pub(crate) fn activity_payload(tab_id: &str, activity: &Activity) -> serde_json::Value {
    let mut payload = serde_json::json!({ "tab_id": tab_id, "verb": activity.verb });
    if let Some(actor) = &activity.actor {
        payload["actor"] = serde_json::Value::String(actor.clone());
    }
    payload
}

fn emit(events: &EventBroadcaster, app: &tauri::AppHandle, tab_id: &str, activity: &Activity) {
    events.publish(SystemEvent::SurfaceActivity {
        tab_id: tab_id.to_string(),
        verb: activity.verb.clone(),
        actor: activity.actor.clone(),
    });
    let _ = app.emit(SURFACE_ACTIVITY_EVENT, activity_payload(tab_id, activity));
}

impl ServiceContext {
    /// One agent verb against a surface tab; bounded to ~4 events per second per tab.
    pub(crate) fn note_surface_activity(&self, tab_id: &str, verb: &str, actor: Option<&str>) {
        let activity = Activity {
            verb: sanitize_verb(verb),
            actor: actor.and_then(sanitize_actor),
        };
        match self
            .surface_activity
            .offer(tab_id, &activity, Instant::now())
        {
            Gate::EmitNow => emit(&self.events, &self.app_handle, tab_id, &activity),
            Gate::Coalesced => {}
            Gate::FlushAfter(delay) => {
                let gate = Arc::clone(&self.surface_activity);
                let events = Arc::clone(&self.events);
                let app = self.app_handle.clone();
                let tab_id = tab_id.to_string();
                std::thread::spawn(move || {
                    std::thread::sleep(delay);
                    if let Some(activity) = gate.flush(&tab_id, Instant::now()) {
                        emit(&events, &app, &tab_id, &activity);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verb(verb: &str) -> Activity {
        Activity {
            verb: verb.into(),
            actor: None,
        }
    }

    #[test]
    fn surface_activity_actor_is_one_bounded_line() {
        assert_eq!(sanitize_actor(" QA "), Some("QA".into()));
        assert_eq!(sanitize_actor("Q\nA\u{7}"), Some("Q A".into()));
        assert_eq!(sanitize_actor(&"x".repeat(80)).unwrap().len(), MAX_ACTOR);
        assert_eq!(sanitize_actor(" \t "), None);
    }

    #[test]
    fn surface_activity_payload_names_a_foreign_actor_only() {
        assert_eq!(
            activity_payload("t", &verb("click")),
            serde_json::json!({ "tab_id": "t", "verb": "click" })
        );
        let foreign = Activity {
            verb: "click".into(),
            actor: Some("QA".into()),
        };
        assert_eq!(
            activity_payload("t", &foreign),
            serde_json::json!({ "tab_id": "t", "verb": "click", "actor": "QA" })
        );
        let event = serde_json::to_value(SystemEvent::SurfaceActivity {
            tab_id: "t".into(),
            verb: "click".into(),
            actor: Some("QA".into()),
        })
        .unwrap();
        assert_eq!(event["actor"], "QA");
    }

    #[test]
    fn surface_activity_trailing_flush_keeps_the_actor_of_the_latest_verb() {
        let gate = SurfaceActivityGate::default();
        let start = Instant::now();
        let foreign = Activity {
            verb: "type".into(),
            actor: Some("QA".into()),
        };
        assert_eq!(gate.offer("t", &verb("goto"), start), Gate::EmitNow);
        let _ = gate.offer("t", &foreign, start + Duration::from_millis(50));
        assert_eq!(
            gate.flush("t", start + Duration::from_millis(250)),
            Some(foreign)
        );
    }

    #[test]
    fn surface_activity_verb_is_a_bare_capped_token() {
        assert_eq!(sanitize_verb("goto"), "goto");
        assert_eq!(sanitize_verb(" Snapshot "), "snapshot");
        assert_eq!(sanitize_verb("profiles.use"), "profiles.use");
        assert_eq!(sanitize_verb("https://mail.example.com/x"), "");
        assert_eq!(sanitize_verb("type hunter2"), "");
        assert_eq!(sanitize_verb("ünicode"), "");
        assert_eq!(sanitize_verb(&"x".repeat(33)), "");
        assert_eq!(sanitize_verb(&"x".repeat(32)), "x".repeat(32));
        assert_eq!(sanitize_verb(""), "");
    }

    #[test]
    fn surface_activity_excludes_ui_calls() {
        for method in [
            "resize",
            "input",
            "state",
            "profiles.list",
            "grab.hover",
            "grab.send",
            "snip.capture",
            "snip.send",
        ] {
            assert!(!is_agent_method(method), "{method}");
        }
        for method in ["nav", "screenshot", "profiles.use"] {
            assert!(is_agent_method(method), "{method}");
        }
    }

    #[test]
    fn surface_activity_coalesces_to_one_trailing_latest_verb_per_window() {
        let gate = SurfaceActivityGate::default();
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        assert_eq!(gate.offer("t", &verb("goto"), at(0)), Gate::EmitNow);
        assert_eq!(
            gate.offer("t", &verb("click"), at(100)),
            Gate::FlushAfter(Duration::from_millis(150))
        );
        assert_eq!(gate.offer("t", &verb("type"), at(200)), Gate::Coalesced);
        assert_eq!(
            gate.offer("other", &verb("snapshot"), at(200)),
            Gate::EmitNow
        );
        assert_eq!(gate.flush("t", at(250)), Some(verb("type")));
        assert_eq!(
            gate.offer("t", &verb("click"), at(300)),
            Gate::FlushAfter(Duration::from_millis(200))
        );
        assert_eq!(gate.flush("t", at(500)), Some(verb("click")));
        assert_eq!(gate.offer("t", &verb("goto"), at(900)), Gate::EmitNow);
        assert_eq!(gate.flush("t", at(950)), None);
    }

    #[test]
    fn surface_activity_stays_within_four_events_per_second() {
        let gate = SurfaceActivityGate::default();
        let start = Instant::now();
        let mut emitted = 0;
        let mut flush_at: Option<Instant> = None;
        for ms in (0..1000).step_by(10) {
            let now = start + Duration::from_millis(ms);
            if flush_at.is_some_and(|due| now >= due) {
                emitted += usize::from(gate.flush("t", now).is_some());
                flush_at = None;
            }
            match gate.offer("t", &verb("click"), now) {
                Gate::EmitNow => emitted += 1,
                Gate::FlushAfter(delay) => flush_at = Some(now + delay),
                Gate::Coalesced => {}
            }
        }
        assert!(emitted <= 4, "{emitted} events in one second");
        assert!(emitted >= 3, "{emitted} events in one second");
    }
}
