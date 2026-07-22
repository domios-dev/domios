// Run: npx tsx src/store/multiplexer/reducer.test.ts
// SET_STATE monotonic generation guard (T3 Fix B): a snapshot whose generation

import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Notification, Workspace, Pane, Tab, Team } from "../../lib/types";
import { TitleLevel } from "../../lib/types";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function base(): ExtendedState {
  return {
    workspaces: [],
    panes: {},
    teams: [],
    active_workspace_id: null,
    focused_pane_id: null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
  };
}

function snap(generation: number, wsId: string): MultiplexerState {
  const ws = {
    id: wsId,
    name: "W",
    tabs: [],
    active_tab_id: null,
    cwd: null,
  } as unknown as Workspace;
  return {
    workspaces: [ws],
    panes: {},
    teams: [],
    active_workspace_id: wsId,
    focused_pane_id: null,
    generation,
  };
}

function snapWithTeams(generation: number, teams: Team[]): MultiplexerState {
  return {
    ...snap(generation, "w-team"),
    teams,
  };
}

test("SET_STATE copies the exact incoming teams array", () => {
  const teams: Team[] = [{
    id: "team-1",
    name: "Launch",
    lifecycle: "forming",
    created_at: 1,
    workspace_id: "w-team",
  }];
  const next = reducer(base(), { type: "SET_STATE", state: snapWithTeams(1, teams) });
  assert(next.teams === teams, "incoming teams array is retained exactly");
});

test("SET_STATE removes orphaned and duplicate tabs before the sidebar can count them", () => {
  const workspace = {
    id: "w-clean",
    name: "Clean",
    tabs: [
      { id: "tab-live", pane_id: "pane-live" },
      { id: "tab-duplicate-pane", pane_id: "pane-live" },
      { id: "tab-orphan", pane_id: "pane-closed" },
    ],
    active_tab_id: "tab-orphan",
    cwd: null,
  } as unknown as Workspace;
  const incoming: MultiplexerState = {
    workspaces: [workspace],
    panes: { "pane-live": { id: "pane-live", title: "Live" } as Pane },
    teams: [],
    active_workspace_id: "w-clean",
    focused_pane_id: null,
    generation: 1,
  };

  const next = reducer(base(), { type: "SET_STATE", state: incoming });
  assert(next.workspaces[0].tabs.map((tab) => tab.id).join(",") === "tab-live", "only the live tab survives");
  assert(next.workspaces[0].active_tab_id === "tab-live", "active tab falls back to the surviving tab");
});

test("SET_STATE replaces teams on a higher generation without retaining removed entries", () => {
  const initial = reducer(base(), {
    type: "SET_STATE",
    state: snapWithTeams(1, [
      { id: "team-1", name: "Before rename", lifecycle: "forming", created_at: 1, workspace_id: "w-team" },
      { id: "team-removed", name: "Removed", lifecycle: "active", created_at: 2, workspace_id: "w-team" },
    ]),
  });
  const incoming: Team[] = [
    { id: "team-1", name: "Renamed", lifecycle: "active", created_at: 1, workspace_id: "w-team" },
  ];
  const next = reducer(initial, { type: "SET_STATE", state: snapWithTeams(2, incoming) });
  const teams = next.teams;
  assert(teams === incoming, "higher generation replaces the complete teams registry");
  assert(teams?.length === 1 && teams[0].name === "Renamed", "rename becomes observable and removed team stays absent");
});

test("SET_STATE rejects stale generations without resurrecting teams", () => {
  const applied = reducer(base(), {
    type: "SET_STATE",
    state: snapWithTeams(2, [{ id: "team-1", name: "Current", lifecycle: "active", created_at: 1, workspace_id: "w-team" }]),
  });
  const after = reducer(applied, {
    type: "SET_STATE",
    state: snapWithTeams(1, [{ id: "team-stale", name: "Stale", lifecycle: "forming", created_at: 2, workspace_id: "w-team" }]),
  });
  assert(after === applied, "stale snapshot returns the applied state reference");
  assert(after.teams[0].id === "team-1", "stale teams cannot resurrect");
  const duplicate = reducer(after, {
    type: "SET_STATE",
    state: snapWithTeams(2, [{ id: "team-duplicate", name: "Duplicate", lifecycle: "forming", created_at: 3, workspace_id: "w-team" }]),
  });
  assert(duplicate === after, "duplicate snapshot returns the applied state reference");
  assert(duplicate.teams[0].id === "team-1", "duplicate teams cannot overwrite the registry");
});

test("deleting a notification removes its history entry and active toast", () => {
  const item: Notification = {
    id: "notice-1",
    pane_id: "tab-pane-1",
    workspace_id: "workspace-1",
    title: "Needs attention",
    body: "Read the full message",
    level: "Warning",
    timestamp: 1,
    read: false,
  };
  const state = { ...base(), notifications: [item], toasts: [item] };
  const next = reducer(state, { type: "DELETE_NOTIFICATION", id: item.id });
  assert(next.notifications.length === 0, "history entry removed");
  assert(next.toasts.length === 0, "active toast removed");
});

test("notification hydration keeps newer live entries and restores backend history", () => {
  const historical: Notification = {
    id: "history-1",
    pane_id: null,
    workspace_id: null,
    title: "Historical",
    body: null,
    level: "Info",
    timestamp: 1,
    read: false,
  };
  const live = { ...historical, id: "live-1", title: "Live" };
  const state = { ...base(), notifications: [live] };
  const next = reducer(state, {
    type: "HYDRATE_NOTIFICATIONS",
    notifications: [historical, { ...live, title: "Stale backend copy" }],
  });
  assert(next.notifications.length === 2, "history and live entries should be deduplicated by id");
  assert(next.notifications.find((item) => item.id === live.id)?.title === live.title, "live event wins a hydration race");
});

test("a newer snapshot is applied", () => {
  const next = reducer(base(), { type: "SET_STATE", state: snap(5, "w-new") });
  assert(next.generation === 5, "generation advances to 5");
  assert(next.active_workspace_id === "w-new", "workspace applied");
});

test("a stale (older generation) snapshot is ignored", () => {
  const applied = reducer(base(), { type: "SET_STATE", state: snap(5, "w-new") });
  const after = reducer(applied, { type: "SET_STATE", state: snap(3, "w-stale") });
  assert(after === applied, "stale snapshot returns the same state reference");
  assert(after.active_workspace_id === "w-new", "stale snapshot does not clobber");
  assert(after.generation === 5, "applied generation unchanged");
});

test("an equal-generation snapshot is ignored", () => {
  const applied = reducer(base(), { type: "SET_STATE", state: snap(5, "w-new") });
  const after = reducer(applied, { type: "SET_STATE", state: snap(5, "w-dup") });
  assert(after === applied, "equal generation returns the same state");
  assert(after.active_workspace_id === "w-new", "equal generation does not clobber");
});

test("snapshots without a generation still apply (back-compat)", () => {
  const noGen = { ...snap(0, "w-x") } as MultiplexerState;
  delete (noGen as { generation?: number }).generation;
  const next = reducer(base(), { type: "SET_STATE", state: noGen });
  assert(next.active_workspace_id === "w-x", "ungenerationed snapshot applied");
});

test("peer switch reset removes every old-peer pane and selection immediately", () => {
  const oldPane = { id: "blackstar-pane", title: "Planner" } as unknown as Pane;
  const oldTab = { id: "blackstar-tab", pane_id: oldPane.id } as unknown as Tab;
  const oldWorkspace = {
    id: "blackstar-workspace",
    name: "BLACKSTAR",
    tabs: [oldTab],
    active_tab_id: oldTab.id,
    cwd: null,
  } as unknown as Workspace;
  const start: ExtendedState = {
    ...base(),
    workspaces: [oldWorkspace],
    panes: { [oldPane.id]: oldPane },
    active_workspace_id: oldWorkspace.id,
    focused_pane_id: oldPane.id,
    paneStatuses: { [oldPane.id]: "Working" },
    activeTabIds: { [oldWorkspace.id]: oldTab.id },
    generation: 42,
  };

  const reset = reducer(start, { type: "RESET_PEER_CONTEXT" });
  assert(reset.workspaces.length === 0, "old peer workspaces cleared");
  assert(Object.keys(reset.panes).length === 0, "old peer panes cleared");
  assert(reset.active_workspace_id === null, "old active workspace cleared");
  assert(reset.focused_pane_id === null, "old focused pane cleared");
  assert(Object.keys(reset.activeTabIds).length === 0, "old active tabs cleared");
  assert(reset.generation === -1, "new peer snapshot cannot be rejected by old peer generation");
});

test("SET_STATE seeds paneStatuses from the snapshot (reload hydration)", () => {
  const withStatus = { ...snap(5, "w-new"), pane_statuses: { "p-1": "Waiting" as const } };
  const next = reducer(base(), { type: "SET_STATE", state: withStatus });
  assert(next.paneStatuses["p-1"] === "Waiting", "status hydrated from mux_get_state snapshot");
});

test("SET_STATE merges pane_statuses without dropping untouched live entries", () => {
  const withLive = { ...base(), paneStatuses: { "p-2": "Working" as const } };
  const withStatus = { ...snap(5, "w-new"), pane_statuses: { "p-1": "Waiting" as const } };
  const next = reducer(withLive, { type: "SET_STATE", state: withStatus });
  assert(next.paneStatuses["p-1"] === "Waiting", "new snapshot status applied");
  assert(next.paneStatuses["p-2"] === "Working", "prior live-tracked status preserved");
});



test("end-to-end: hook-driven pending_prompt survives reload as attention (not Working)", () => {
  const promptedPane = {
    id: "p-1",
    pty_id: "pty-1",
    title: "shell",
    pending_prompt: { question: "Allow?", options: [{ label: "Yes", value: "1" }] },
  } as unknown as Pane;
  const withPrompt = {
    ...snap(5, "w-new"),
    panes: { "p-1": promptedPane },
    pane_statuses: { "p-1": "Waiting" as const },
  };
  const next = reducer(base(), { type: "SET_STATE", state: withPrompt });
  assert(next.paneStatuses["p-1"] === "Waiting", "backend-derived Waiting hydrates, not Working");
  assert(Boolean(next.panes["p-1"].pending_prompt), "pending_prompt itself survives the snapshot merge");
});

test("SET_STATE drops a stale Waiting once pending_prompt clears and no fresh status is reported", () => {
  const withWaiting = {
    ...base(),
    panes: {
      "p-1": {
        id: "p-1",
        pty_id: null,
        title: "shell",
        pending_prompt: { question: "Allow?", options: [{ label: "Yes", value: "1" }] },
      } as unknown as Pane,
    },
    paneStatuses: { "p-1": "Waiting" as const },
  };
  const cleared = {
    ...snap(5, "w-new"),
    panes: {
      "p-1": { id: "p-1", pty_id: null, title: "shell" } as unknown as Pane,
    },
    // No pane_statuses entry for p-1 — the pane has no PTY to derive from.
  };
  const next = reducer(withWaiting, { type: "SET_STATE", state: cleared });
  assert(
    next.paneStatuses["p-1"] === undefined,
    "stale Waiting cleared once pending_prompt is gone and no fresh status reports it",
  );
});

test("SET_STATE keeps a genuine tracker-reported Waiting even without pending_prompt", () => {
  const withWaiting = { ...base(), paneStatuses: { "p-1": "Waiting" as const } };
  const stillWaiting = {
    ...snap(5, "w-new"),
    pane_statuses: { "p-1": "Waiting" as const },
  };
  const next = reducer(withWaiting, { type: "SET_STATE", state: stillWaiting });
  assert(next.paneStatuses["p-1"] === "Waiting", "tracker-reaffirmed Waiting is not dropped");
});

test("boot stage events preserve failure reason and clear it on retry and ready", () => {
  const hydrated = {
    ...base(),
    panes: { "p-1": { id: "p-1", pty_id: null, title: "General", protected: true } as unknown as Pane },
  };
  const starting = reducer(hydrated, {
    type: "UPDATE_BOOT_STAGE",
    paneId: "p-1",
    stage: "pty_starting",
  });
  assert(starting.panes["p-1"].boot_stage === "pty_starting", "pty_starting recorded");
  assert(starting.panes["p-1"].boot_failure_reason === null, "retry clears stale reason");

  const failed = reducer(starting, {
    type: "UPDATE_BOOT_STAGE",
    paneId: "p-1",
    stage: "spawn_failed",
    reason: "provider missing",
  });
  assert(failed.panes["p-1"].boot_stage === "spawn_failed", "spawn_failed recorded");
  assert(failed.panes["p-1"].boot_failure_reason === "provider missing", "failure reason recorded");

  const retry = reducer(failed, {
    type: "UPDATE_BOOT_STAGE",
    paneId: "p-1",
    stage: "pty_starting",
  });
  assert(retry.panes["p-1"].boot_stage === "pty_starting", "retry moves back to starting");
  assert(retry.panes["p-1"].boot_failure_reason === null, "stale failure reason cleared on retry");

  const ready = reducer(retry, {
    type: "UPDATE_BOOT_STAGE",
    paneId: "p-1",
    stage: "agent_ready",
  });
  assert(ready.panes["p-1"].boot_stage === "agent_ready", "agent_ready recorded");
  assert(ready.panes["p-1"].boot_failure_reason === null, "ready leaves no stale failure reason");
});

test("late attach hydration carries spawn failure and later snapshot clears it", () => {
  const failedSnapshot = {
    ...snap(7, "w-new"),
    panes: {
      "p-1": {
        id: "p-1",
        pty_id: null,
        title: "General",
        protected: true,
        boot_stage: "spawn_failed",
        boot_failure_reason: "child exited",
      } as unknown as Pane,
    },
  };
  const hydrated = reducer(base(), { type: "SET_STATE", state: failedSnapshot });
  assert(hydrated.panes["p-1"].boot_stage === "spawn_failed", "late attach sees failed stage");
  assert(
    hydrated.panes["p-1"].boot_failure_reason === "child exited",
    "late attach sees failure detail",
  );

  const readySnapshot = {
    ...snap(8, "w-new"),
    panes: {
      "p-1": {
        id: "p-1",
        pty_id: "pty-1",
        title: "General",
        protected: true,
        boot_stage: "agent_ready",
        boot_failure_reason: null,
      } as unknown as Pane,
    },
  };
  const ready = reducer(hydrated, { type: "SET_STATE", state: readySnapshot });
  assert(ready.panes["p-1"].boot_stage === "agent_ready", "new snapshot hydrates ready stage");
  assert(ready.panes["p-1"].boot_failure_reason === null, "new snapshot clears stale failure detail");
});

function pane(id: string): Pane {
  return { id, pty_id: null, title: id } as unknown as Pane;
}

function snapWithFocus(
  generation: number,
  paneIds: string[],
  focused: string | null,
): MultiplexerState {
  const panes: Record<string, Pane> = {};
  for (const id of paneIds) panes[id] = pane(id);
  return {
    workspaces: [],
    panes,
    active_workspace_id: null,
    focused_pane_id: focused,
    generation,
  };
}

test("initial hydrate adopts focused_pane_id naming an existing pane", () => {
  const next = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  assert(next.focused_pane_id === "p1", "focus adopted from snapshot");
});

test("snapshot naming a missing pane preserves prior focus", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p1"], "ghost"),
  });
  assert(
    after.focused_pane_id === "p1",
    "stale focus naming a missing pane does not clobber valid focus",
  );
});

test("prior focused pane removed from snapshot clears focused_pane_id to null", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p2"], null),
  });
  assert(after.focused_pane_id === null, "ghost focused_pane_id cleared when pane removed");
});

test("snapshot with null focus preserves prior focus", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p1"], null),
  });
  assert(after.focused_pane_id === "p1", "null focus does not clobber valid focus");
});

function paneWithTitle(id: string, title: string, titleLevel?: TitleLevel): Pane {
  return {
    id,
    pty_id: null,
    title,
    title_level: titleLevel,
  } as unknown as Pane;
}

test("SET_STATE drops frontend Auto title in favor of backend Default", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: "p1",
      generation: 1,
    },
  });
  const withAuto = reducer(hydrated, {
    type: "SET_PANE_TITLE",
    paneId: "p1",
    title: "codeterm",
    level: TitleLevel.Auto,
  });
  assert(withAuto.panes.p1.title === "codeterm", "local Auto title applied");
  const after = reducer(withAuto, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 2,
    },
  });
  assert(after.panes.p1.title === "Tab 3", "backend title wins over frontend Auto on snapshot");
});

test("SET_STATE keeps frontend User title until backend catches up", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: "p1",
      generation: 1,
    },
  });
  const userPinned = reducer(hydrated, {
    type: "SET_PANE_TITLE",
    paneId: "p1",
    title: "My Worker",
    level: TitleLevel.User,
  });
  const after = reducer(userPinned, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 2,
    },
  });
  assert(after.panes.p1.title === "My Worker", "User rename survives snapshot until backend sync");
});

test("SET_PANE_TITLE rejects lower priority overwrite", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Pinned", TitleLevel.User) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 1,
    },
  });
  const after = reducer(hydrated, {
    type: "SET_PANE_TITLE",
    paneId: "p1",
    title: "osc-title",
    level: TitleLevel.Auto,
  });
  assert(after === hydrated, "Auto cannot overwrite User title");
  assert(after.panes.p1.title === "Pinned", "User title unchanged");
});

test("SET_STATE yields to a fresh backend Default rename over a stale carried-over Default title", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "stale-cached-name", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 1,
    },
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "fresh-daemon-name", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 2,
    },
  });
  assert(
    after.panes.p1.title === "fresh-daemon-name",
    "stale cached Default title yields to the backend's fresh Default rename",
  );
});

test("SET_STATE yields to a fresh backend User rename over a title held across a reconnect outage", () => {
  const beforeOutage = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "before-outage", TitleLevel.User) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 1,
    },
  });
  const afterReconnect = reducer(beforeOutage, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "renamed-during-outage", TitleLevel.User) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 2,
    },
  });
  assert(
    afterReconnect.panes.p1.title === "renamed-during-outage",
    "title held across a reconnect outage yields to the backend's authoritative rename",
  );
});

test("a genuine pending-local User rename survives an interim snapshot, then converges once backend catches up", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 1,
    },
  });
  const userPinned = reducer(hydrated, {
    type: "SET_PANE_TITLE",
    paneId: "p1",
    title: "My Worker",
    level: TitleLevel.User,
  });
  const interim = reducer(userPinned, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 2,
    },
  });
  assert(interim.panes.p1.title === "My Worker", "pending local rename survives an interim backend snapshot");
  const confirmed = reducer(interim, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "My Worker", TitleLevel.User) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 3,
    },
  });
  assert(confirmed.panes.p1.title === "My Worker", "title matches once backend catches up");
  const renamedElsewhere = reducer(confirmed, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Renamed Elsewhere", TitleLevel.User) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 4,
    },
  });
  assert(
    renamedElsewhere.panes.p1.title === "Renamed Elsewhere",
    "pending flag cleared on confirmation, so a later tie yields to the backend again",
  );
});

test("SET_PANE_TITLE at User level pins title for tab rename round-trip", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      panes: { p1: paneWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_pane_id: null,
      generation: 1,
    },
  });
  const renamed = reducer(hydrated, {
    type: "SET_PANE_TITLE",
    paneId: "p1",
    title: "Renamed Tab",
    level: TitleLevel.User,
  });
  assert(renamed.panes.p1.title === "Renamed Tab", "User rename applied on pane");
  assert(renamed.panes.p1.title_pending_local === true, "rename marked pending until backend sync");
});

test("RESET_GENERATION lets a low post-restart snapshot apply over a high stale generation", () => {
  const highGen = reducer(base(), { type: "SET_STATE", state: snap(500, "w-old") });
  assert(highGen.generation === 500, "client accumulated a high generation");
  const resetted = reducer(highGen, { type: "RESET_GENERATION" });
  assert(resetted.generation === 0, "generation dropped to 0 on boot-changed");
  assert(resetted.active_workspace_id === "w-old", "workspace/pane structure untouched by the reset itself");
  const after = reducer(resetted, { type: "SET_STATE", state: snap(1, "w-post-restart") });
  assert(after.generation === 1, "low post-restart generation now applies");
  assert(after.active_workspace_id === "w-post-restart", "post-restart structure replaces stale pre-restart structure");
});

test("without RESET_GENERATION a low post-restart snapshot would be rejected (sanity check)", () => {
  const highGen = reducer(base(), { type: "SET_STATE", state: snap(500, "w-old") });
  const after = reducer(highGen, { type: "SET_STATE", state: snap(1, "w-post-restart") });
  assert(after === highGen, "monotonic guard still rejects a low generation outside the boot-changed path");
});

test("CLOSE_TAB removes every tab whose pane is in the removed cascade", () => {
  const orch: Tab = { id: "tab-orch", pane_id: "pane-orch" } as unknown as Tab;
  const planner: Tab = { id: "tab-planner", pane_id: "pane-planner" } as unknown as Tab;
  const worker: Tab = { id: "tab-worker", pane_id: "pane-worker" } as unknown as Tab;
  const start: ExtendedState = {
    ...base(),
    workspaces: [
      {
        id: "w1",
        name: "W",
        tabs: [orch, planner, worker],
        active_tab_id: "tab-worker",
        cwd: null,
      } as unknown as Workspace,
    ],
    panes: {
      "pane-orch": { id: "pane-orch", title: "Orchestrator" } as unknown as Pane,
      "pane-planner": {
        id: "pane-planner",
        title: "Planner",
        parent_pane_id: "pane-orch",
      } as unknown as Pane,
      "pane-worker": {
        id: "pane-worker",
        title: "Worker",
        parent_pane_id: "pane-planner",
      } as unknown as Pane,
    },
    paneStatuses: {
      "pane-orch": "Idle",
      "pane-planner": "Working",
      "pane-worker": "Waiting",
    },
    activeTabIds: { w1: "tab-worker" },
  };

  const after = reducer(start, {
    type: "CLOSE_TAB",
    workspaceId: "w1",
    tabId: "tab-orch",
    removedPaneIds: ["pane-orch", "pane-planner", "pane-worker"],
  });
  const ws = after.workspaces.find((w) => w.id === "w1")!;
  assert(ws.tabs.length === 0, `all cascade tabs removed, got ${ws.tabs.length}`);
  assert(ws.active_tab_id === null, "workspace active tab cleared");
  assert(after.activeTabIds.w1 === undefined, "activeTabIds cleared");
  assert(Object.keys(after.panes).length === 0, "all cascade panes removed");
  assert(Object.keys(after.paneStatuses).length === 0, "all cascade statuses removed");
});

test("CLOSE_TAB removes a manager-root subtree", () => {
  const orch: Tab = { id: "tab-orch", pane_id: "pane-orch" } as unknown as Tab;
  const manager: Tab = { id: "tab-manager", pane_id: "pane-manager" } as unknown as Tab;
  const worker: Tab = { id: "tab-worker", pane_id: "pane-worker" } as unknown as Tab;
  const start: ExtendedState = {
    ...base(),
    workspaces: [
      {
        id: "w1",
        name: "W",
        tabs: [orch, manager, worker],
        active_tab_id: "tab-worker",
        cwd: null,
      } as unknown as Workspace,
    ],
    panes: {
      "pane-orch": { id: "pane-orch", title: "Orchestrator" } as unknown as Pane,
      "pane-manager": {
        id: "pane-manager",
        title: "Planner",
        role: "Manager",
        parent_pane_id: "pane-orch",
      } as unknown as Pane,
      "pane-worker": {
        id: "pane-worker",
        title: "Worker",
        role: "Worker",
        parent_pane_id: "pane-manager",
      } as unknown as Pane,
    },
    paneStatuses: {
      "pane-manager": "Working",
      "pane-worker": "Waiting",
    },
    activeTabIds: { w1: "tab-worker" },
  };

  const after = reducer(start, {
    type: "CLOSE_TAB",
    workspaceId: "w1",
    tabId: "tab-manager",
    removedPaneIds: ["pane-manager", "pane-worker"],
  });
  const ws = after.workspaces.find((w) => w.id === "w1")!;
  assert(ws.tabs.map((t) => t.id).join(",") === "tab-orch", "only orchestrator remains");
  assert(ws.active_tab_id === "tab-orch", "active tab falls back to orchestrator");
  assert(after.activeTabIds.w1 === "tab-orch", "activeTabIds falls back to orchestrator");
  assert(after.panes["pane-orch"], "orchestrator pane remains");
  assert(!after.panes["pane-manager"] && !after.panes["pane-worker"], "manager subtree panes removed");
  assert(Object.keys(after.paneStatuses).length === 0, "manager subtree statuses removed");
});

console.log(failures === 0 ? "✓ reducer tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
