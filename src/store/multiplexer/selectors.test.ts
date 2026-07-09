// Run: npx tsx src/store/multiplexer/selectors.test.ts

import { strict as assert } from "node:assert";
import type { Pane } from "../../lib/types";
import {
  paneFieldsEqual,
  paneTitleMapEqual,
  panesRecordEqual,
  selectPaneTitleMap,
} from "./selectors";
import type { ExtendedState } from "./tree";

function samplePane(id: string, elapsed?: number): Pane {
  return {
    id,
    pty_id: "pty-1",
    pane_type: "Terminal",
    title: "Agent",
    cwd: null,
    agent_type: "claude",
    agent_state: elapsed == null
      ? null
      : { state: "working", elapsed_ms: elapsed, status_line: `tick ${elapsed}` },
  };
}

function sampleState(panes: Record<string, Pane>): ExtendedState {
  const paneIds = Object.keys(panes);
  return {
    workspaces: [{
      id: "ws",
      name: "ws",
      tabs: paneIds.map((pane_id) => ({ id: `tab-${pane_id}`, pane_id })),
      active_tab_id: paneIds[0] ? `tab-${paneIds[0]}` : null,
      cwd: null,
    }],
    panes,
    active_workspace_id: "ws",
    focused_pane_id: paneIds[0] ?? null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    virtualTabs: {},
  };
}

assert.equal(
  paneFieldsEqual(samplePane("a", 100), samplePane("a", 100)),
  true,
  "same pane fields compare equal",
);
assert.equal(
  paneFieldsEqual(samplePane("a", 100), samplePane("a", 200)),
  false,
  "elapsed_ms change is unequal",
);
assert.equal(
  paneFieldsEqual(samplePane("a"), { ...samplePane("a"), title: "Renamed" }),
  false,
  "title change is unequal",
);

const panesA = { a: samplePane("a", 1), b: samplePane("b", 5) };
const panesB = { a: samplePane("a", 2), b: samplePane("b", 5) };
assert.equal(panesRecordEqual(panesA, panesB), false, "record differs when one pane ticks");
assert.equal(
  panesRecordEqual({ b: samplePane("b", 5) }, { b: samplePane("b", 5) }),
  true,
  "unchanged pane in record compares equal",
);

const titleMap1 = selectPaneTitleMap(sampleState({ x: { ...samplePane("x"), title: "✻ Worker" } }));
const titleMap2 = selectPaneTitleMap(sampleState({ x: { ...samplePane("x"), title: "✻ Worker" } }));
assert.equal(paneTitleMapEqual(titleMap1, titleMap2), true, "title map stable when titles match");

console.log("selectors.test.ts: all passed");
