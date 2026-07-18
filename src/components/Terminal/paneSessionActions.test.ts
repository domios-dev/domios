import assert from "node:assert/strict";
import type { Pane } from "../../lib/types";
import { paneSessionActionView } from "./paneSessionActions";

function pane(overrides: Partial<Pane>): Pane {
  return { id: "pane", pty_id: null, pane_type: "Terminal", title: "Pane", cwd: null, ...overrides };
}

assert.deepEqual(
  paneSessionActionView(pane({ agent_type: "claude", agent_session_id: "agent-1", backing: { kind: "pty" } })),
  { sessionId: "agent-1", canReconnect: true, canRestart: true },
);
assert.deepEqual(
  paneSessionActionView(pane({ backing: { kind: "chat_backend", plugin_id: "lmstudio", session_id: "chat-1" } })),
  { sessionId: "chat-1", canReconnect: true, canRestart: false },
);
assert.deepEqual(
  paneSessionActionView(pane({ backing: { kind: "pty" } })),
  { sessionId: null, canReconnect: false, canRestart: false },
);

console.log("pane session action tests: passed");
