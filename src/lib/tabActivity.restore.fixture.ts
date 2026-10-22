import type { Tab } from "./types";

export const restoredBoundAgent: Tab = {
  id: "restored-agent", title: "Restored", tab_type: "Terminal", cwd: null,
  backing: { kind: "pty" }, agent_type: "fixture",
  agent_session_id: "owned-restored-session", pty_id: "restored-pty",
  session_binding_nonce: "current-launch", reported_session_binding_nonce: "current-launch",
  agent_booting: false, frozen: false, boot_stage: "pty_starting",
  agent_state: { state: "working", phase: "work" },
};
