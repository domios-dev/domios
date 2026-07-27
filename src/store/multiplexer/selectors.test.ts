// Run: npx tsx src/store/multiplexer/selectors.test.ts

import { strict as assert } from "node:assert";
import type { Tab, Team } from "../../lib/types";
import {
  tabFieldsEqual,
  tabTitleMapEqual,
  tabsRecordEqual,
  selectTabTitleMap,
  selectTeams,
  teamsEqual,
} from "./selectors";
import type { ExtendedState } from "./tree";

function sampleTab(id: string, elapsed?: number): Tab {
  return {
    id,
    pty_id: "pty-1",
    tab_type: "Terminal",
    title: "Agent",
    cwd: null,
    agent_type: "claude",
    agent_state: elapsed == null
      ? null
      : { state: "working", elapsed_ms: elapsed, status_line: `tick ${elapsed}` },
  };
}

function sampleState(tabs: Record<string, Tab>, teams: Team[] = []): ExtendedState {
  const tabIds = Object.keys(tabs);
  return {
    workspaces: [{
      id: "ws",
      name: "ws",
      tabs: tabIds.map((tab_id) => ({ id: `tab-${tab_id}`, tab_id })),
      active_tab_id: tabIds[0] ? `tab-${tabIds[0]}` : null,
      cwd: null,
    }],
    tabs,
    teams,
    active_workspace_id: "ws",
    focused_tab_id: tabIds[0] ?? null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 1,
    pendingCloses: [],
  };
}

assert.equal(
  tabFieldsEqual(sampleTab("a", 100), sampleTab("a", 100)),
  true,
  "same Tab fields compare equal",
);
assert.equal(
  tabFieldsEqual(sampleTab("a", 100), sampleTab("a", 200)),
  false,
  "elapsed_ms change is unequal",
);
assert.equal(
  tabFieldsEqual(sampleTab("a"), { ...sampleTab("a"), title: "Renamed" }),
  false,
  "title change is unequal",
);

const tabsA = { a: sampleTab("a", 1), b: sampleTab("b", 5) };
const tabsB = { a: sampleTab("a", 2), b: sampleTab("b", 5) };
assert.equal(tabsRecordEqual(tabsA, tabsB), false, "record differs when one Tab ticks");
assert.equal(
  tabsRecordEqual({ b: sampleTab("b", 5) }, { b: sampleTab("b", 5) }),
  true,
  "unchanged Tab in record compares equal",
);

const titleMap1 = selectTabTitleMap(sampleState({ x: { ...sampleTab("x"), title: "✻ Worker" } }));
const titleMap2 = selectTabTitleMap(sampleState({ x: { ...sampleTab("x"), title: "✻ Worker" } }));
assert.equal(tabTitleMapEqual(titleMap1, titleMap2), true, "title map stable when titles match");

const formingTeam: Team = {
  id: "team-1",
  name: "Launch",
  lifecycle: "forming",
  created_at: 1,
  workspace_id: "ws",
};
const activeTeam = { ...formingTeam, name: "Launch renamed", lifecycle: "active" as const };
const teamsBefore = selectTeams(sampleState({}, [formingTeam]));
const teamsAfter = selectTeams(sampleState({}, [activeTeam]));
assert.equal(teamsBefore[0].name, "Launch", "teams selector exposes the snapshot name");
assert.equal(teamsAfter[0].lifecycle, "active", "teams selector exposes lifecycle changes");
assert.equal(teamsEqual(teamsBefore, teamsAfter), false, "teams equality does not mask a renamed lifecycle change");

console.log("selectors.test.ts: all passed");
