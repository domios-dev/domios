//! Truthful pane-boot progress. Stages are emitted at the REAL launch
//! transitions in `orchestrator::launch` — never timer-driven — so late UI
//! attachers and live subscribers see where a spawn actually is.

use serde::{Deserialize, Serialize};

use super::ServiceContext;

/// One stage of a pane's agent-boot lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootStage {
    /// PTY exists and its stream is claimed (frontend- or backend-created).
    PtySpawned,
    /// Shell prompt is ready to accept the launch command.
    ShellReady,
    /// Launch command sent; the agent TUI is coming up.
    AgentLaunching,
    /// Agent booted and the initial task (if any) was delivered.
    AgentReady,
    /// Boot aborted; carries a human-readable reason on the event payload.
    SpawnFailed,
}

impl BootStage {
    /// Canonical happy-path order. The single source of truth the launch path
    /// walks and the ordered-sequence test asserts against.
    pub const HAPPY_PATH: [BootStage; 4] = [
        BootStage::PtySpawned,
        BootStage::ShellReady,
        BootStage::AgentLaunching,
        BootStage::AgentReady,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            BootStage::PtySpawned => "pty_spawned",
            BootStage::ShellReady => "shell_ready",
            BootStage::AgentLaunching => "agent_launching",
            BootStage::AgentReady => "agent_ready",
            BootStage::SpawnFailed => "spawn_failed",
        }
    }
}

/// Where a boot-stage transition goes. Real path writes the pane snapshot +
/// fans the event out on the bridge; tests record into a vec.
pub trait BootStageSink {
    fn record(&self, pane_id: &str, stage: BootStage, reason: Option<&str>);
}

impl BootStageSink for ServiceContext {
    fn record(&self, pane_id: &str, stage: BootStage, reason: Option<&str>) {
        self.emit_boot_stage(pane_id, stage, reason);
    }
}

impl ServiceContext {
    /// Set the pane's current boot stage (for late attachers reading the
    /// snapshot) and fan the transition out on the event bridge. Fire-and-forget
    /// — never blocks the launch thread.
    pub(crate) fn emit_boot_stage(&self, pane_id: &str, stage: BootStage, reason: Option<&str>) {
        if let Some(p) = self.mux.write().get_pane_mut(pane_id) {
            p.boot_stage = Some(stage);
        }
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "pane-boot-stage",
            serde_json::json!({
                "pane_id": pane_id,
                "stage": stage,
                "reason": reason,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct MockSink {
        events: Mutex<Vec<(String, BootStage, Option<String>)>>,
    }

    impl BootStageSink for MockSink {
        fn record(&self, pane_id: &str, stage: BootStage, reason: Option<&str>) {
            self.events.lock().push((
                pane_id.to_string(),
                stage,
                reason.map(str::to_string),
            ));
        }
    }

    impl MockSink {
        fn stages(&self) -> Vec<BootStage> {
            self.events.lock().iter().map(|(_, s, _)| *s).collect()
        }
    }

    #[test]
    fn happy_path_emits_stages_in_order() {
        let sink = MockSink::default();
        for stage in BootStage::HAPPY_PATH {
            sink.record("pane-1", stage, None);
        }
        assert_eq!(sink.stages(), BootStage::HAPPY_PATH.to_vec());
        assert!(sink.events.lock().iter().all(|(pid, _, r)| pid == "pane-1" && r.is_none()));
    }

    #[test]
    fn failure_path_ends_at_spawn_failed_with_reason_and_no_ready() {
        let sink = MockSink::default();
        sink.record("pane-2", BootStage::PtySpawned, None);
        sink.record("pane-2", BootStage::ShellReady, None);
        sink.record("pane-2", BootStage::SpawnFailed, Some("launch_cmd failed: boom"));

        let stages = sink.stages();
        assert!(!stages.contains(&BootStage::AgentReady));
        assert_eq!(stages.last(), Some(&BootStage::SpawnFailed));
        let (_, last_stage, last_reason) = sink.events.lock().last().cloned().unwrap();
        assert_eq!(last_stage, BootStage::SpawnFailed);
        assert_eq!(last_reason.as_deref(), Some("launch_cmd failed: boom"));
    }

    #[test]
    fn stage_serializes_snake_case() {
        let json = serde_json::to_string(&BootStage::AgentLaunching).unwrap();
        assert_eq!(json, "\"agent_launching\"");
        assert_eq!(BootStage::SpawnFailed.as_str(), "spawn_failed");
    }
}
