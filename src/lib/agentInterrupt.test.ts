import assert from "node:assert/strict";
import test from "node:test";
import { interruptAgentArgs, showStopControl } from "./agentInterrupt";
import { tabActivity } from "./tabActivity";
import type { Tab, TabAgentState } from "./types";

const runtimeTab = { id: "runtime", pty_id: null, backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "thread" } } as Tab;
const ptyTab = { id: "pty-tab", pty_id: "pty", tab_type: "Terminal", agent_type: "fixture", agent_session_id: "session" } as Tab;
const unroutedTab = { ...ptyTab, id: "unrouted", pty_id: null } as Tab;

const states: Array<[string, TabAgentState, boolean]> = [
  ["working", { state: "working" }, true],
  ["idle", { state: "idle" }, false],
  ["compacting", { state: "working", status_line: "Compacting context" }, true],
  ["running tool", { state: "working", phase: "tool" }, true],
  ["question", { state: "question" }, false],
];

test("ordinary tab interruption retains provider authority", () => {
  assert.deepEqual(interruptAgentArgs({ id: "ordinary", pty_id: "pty" }), { ptyId: "pty", tabId: "ordinary" });
});

test("runtime tabs interrupt through the tab stop route", () => {
  assert.deepEqual(interruptAgentArgs(runtimeTab), { tabId: "runtime" });
});

test("tabs without a PTY or runtime cannot interrupt", () => {
  assert.equal(interruptAgentArgs({ id: "ordinary", pty_id: null }), null);
});

for (const [name, agentState, expected] of states) {
  test(`Stop for a ${name} agent follows its state on PTY and runtime tabs alike`, () => {
    for (const tab of [runtimeTab, ptyTab]) {
      assert.equal(showStopControl(tabActivity({ ...tab, agent_state: agentState }), tab), expected, `${name} ${tab.id}`);
    }
  });
}

test("Stop stays hidden without an interrupt route even while working", () => {
  assert.equal(showStopControl(tabActivity({ ...unroutedTab, agent_state: { state: "working" } }), unroutedTab), false);
});
