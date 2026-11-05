use super::ServiceContext;
use codeterm_util::memory_size::estimate;
use serde_json::{json, Value};

impl ServiceContext {
    pub fn memory_diagnostics(&self) -> Value {
        let started = std::time::Instant::now();
        let mux = self.mux.try_read().map(|mux| {
            let tabs: Vec<_> = mux.all_tabs().map(|tab| json!({
                "tab_id": tab.id,
                "pty_id": tab.pty_id,
                "approx_bytes": estimate(tab),
            })).collect();
            json!({
                "count": tabs.len(),
                "approx_bytes": tabs.iter().filter_map(|tab| tab["approx_bytes"].as_u64()).sum::<u64>(),
                "tabs": tabs,
            })
        }).unwrap_or_else(|| json!({"busy": true}));
        let output = self
            .output_buf
            .try_lock()
            .map(|output| output.memory_stats())
            .unwrap_or_else(|| json!({"busy": true}));
        let screen = self.screen_state.memory_stats();
        let notifications = self
            .notif
            .try_lock()
            .map(|store| store.memory_stats())
            .unwrap_or_else(|| json!({"busy": true}));
        let blueprints = self
            .try_ext::<crate::ports::sentinel::SentinelPorts>()
            .map(|ports| ports.store.memory_diagnostics())
            .unwrap_or_else(|| json!({"available": false}));
        let run_map = self
            .agent_run_pty_map
            .try_lock()
            .map(|map| {
                json!({
                    "count": map.len(),
                    "approx_bytes": estimate(&*map),
                })
            })
            .unwrap_or_else(|| json!({"busy": true}));
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            true,
            sysinfo::ProcessRefreshKind::nothing()
                .without_tasks()
                .with_memory(),
        );
        json!({
            "pid": std::process::id(),
            "process_resident_bytes": system.process(pid).map(sysinfo::Process::memory),
            "process_virtual_bytes": system.process(pid).map(sysinfo::Process::virtual_memory),
            "measurement": "approximate serialized field storage; excludes allocator overhead, spare capacity and skipped fields; cache charges and PTY allocations use their own accounting",
            "limitations": "not an allocator census; model weight file sizes and screen upper bounds are not RSS; shared allocations may overlap; busy subsystems are skipped",
            "subsystems": {
                "tabs": mux,
                "pty_output": output,
                "pty_screens": screen,
                "chat": self.chat_registry.memory_stats(),
                "chat_history_writers": super::tab_chat::chatbackend_history::memory_stats(),
                "memory": self.mem.memory_diagnostics(),
                "blueprints": blueprints,
                "notifications": notifications,
                "agent_run_pty_map": run_map,
            },
            "elapsed_us": started.elapsed().as_micros(),
        })
    }
}
