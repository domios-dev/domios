import assert from "node:assert/strict";
import type { Tab } from "../../lib/types";
import { sameRowTab } from "./tabRowEquality";

const base = {
  id: "t1",
  title: "Cook",
  agent_type: "claude",
  agent_state: { state: "working", status_line: "Writing", elapsed_ms: 1000 },
  backing: { kind: "chat_backend", plugin_id: "codex", session_id: "s1" },
  last_active_at_ms: 10,
} as unknown as Tab;

const variant = (patch: Record<string, unknown>) => ({ ...base, ...patch }) as unknown as Tab;

assert.equal(sameRowTab(base, base), true, "the same snapshot is the same row");
assert.equal(sameRowTab(base, variant({ provider_signals: { background: { running: { child: "Inspect" } } } })), false, "background roster updates reach the row");
assert.equal(sameRowTab(base, variant({ last_active_at_ms: 99 })), true, "activity timestamps do not re-render a row");
assert.equal(sameRowTab(base, variant({ agent_state: { ...base.agent_state, elapsed_ms: 5000 } })), true, "elapsed timers do not re-render a row");
assert.equal(sameRowTab(base, variant({ title: "Chef" })), false, "a new title re-renders the row");
assert.equal(sameRowTab(base, variant({ agent_state: { ...base.agent_state, state: "idle" } })), false, "a new agent state re-renders the row");
assert.equal(sameRowTab(base, variant({ backing: { kind: "chat_backend", plugin_id: "codex", session_id: "s2" } })), false, "a rebound chat session re-renders the row");
assert.equal(sameRowTab(base, variant({ connected_services: ["telegram"] })), false, "connected services re-render the row");
