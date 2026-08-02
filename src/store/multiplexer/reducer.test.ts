// Run: npx tsx src/store/multiplexer/reducer.test.ts
// SET_STATE monotonic generation guard (T3 Fix B): a snapshot whose generation

import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Notification, Workspace, Tab, Team } from "../../lib/types";
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
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
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
    tabs: {},
    teams: [],
    active_workspace_id: wsId,
    focused_tab_id: null,
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
    tabs: ["tab-live", "tab-live", "tab-closed"],
    active_tab_id: "tab-orphan",
    cwd: null,
  } as unknown as Workspace;
  const incoming: MultiplexerState = {
    workspaces: [workspace],
    tabs: { "tab-live": { id: "tab-live", title: "Live" } as Tab },
    teams: [],
    active_workspace_id: "w-clean",
    focused_tab_id: null,
    generation: 1,
  };

  const next = reducer(base(), { type: "SET_STATE", state: incoming });
  assert(next.workspaces[0].tabs.join(",") === "tab-live", "only the live tab survives");
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
    tab_id: "tab-tab-1",
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
    tab_id: null,
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

test("peer switch reset removes every old-peer Tab and selection immediately", () => {
  const oldTab = { id: "blackstar-tab", title: "Planner" } as unknown as Tab;
  const oldWorkspace = {
    id: "blackstar-workspace",
    name: "BLACKSTAR",
    tabs: [oldTab.id],
    active_tab_id: oldTab.id,
    cwd: null,
  } as unknown as Workspace;
  const start: ExtendedState = {
    ...base(),
    workspaces: [oldWorkspace],
    tabs: { [oldTab.id]: oldTab },
    active_workspace_id: oldWorkspace.id,
    focused_tab_id: oldTab.id,
    tabStatuses: { [oldTab.id]: "Working" },
    activeTabIds: { [oldWorkspace.id]: oldTab.id },
    generation: 42,
  };

  const reset = reducer(start, { type: "RESET_PEER_CONTEXT" });
  assert(reset.workspaces.length === 0, "old peer workspaces cleared");
  assert(Object.keys(reset.tabs).length === 0, "old peer Tabs cleared");
  assert(reset.active_workspace_id === null, "old active workspace cleared");
  assert(reset.focused_tab_id === null, "old focused Tab cleared");
  assert(Object.keys(reset.activeTabIds).length === 0, "old active tabs cleared");
  assert(reset.generation === -1, "new peer snapshot cannot be rejected by old peer generation");
});

test("SET_REMOTE_TABS places a remote tab in its controller workspace", () => {
  const workspaceA = { id: "workspace-a", tabs: [], active_tab_id: null } as unknown as Workspace;
  const workspaceB = { id: "workspace-b", tabs: [], active_tab_id: null } as unknown as Workspace;
  const remote = {
    id: "mesh:peer/worker",
    title: "Worker",
    remote_peer_id: "peer",
    remote_tab_id: "worker",
    workspace_id: "workspace-b",
  } as unknown as Tab;
  const next = reducer(
    { ...base(), workspaces: [workspaceA, workspaceB] },
    { type: "SET_REMOTE_TABS", tabs: { [remote.id]: remote } },
  );
  assert(next.workspaces[0].tabs.length === 0, "unrelated first workspace stays unchanged");
  assert(next.workspaces[1].tabs[0] === remote.id, "remote tab follows its controller workspace");
});

test("SET_STATE seeds tabStatuses from the snapshot (reload hydration)", () => {
  const withStatus = { ...snap(5, "w-new"), tab_statuses: { "p-1": "Waiting" as const } };
  const next = reducer(base(), { type: "SET_STATE", state: withStatus });
  assert(next.tabStatuses["p-1"] === "Waiting", "status hydrated from mux_get_state snapshot");
});

test("SET_STATE merges tab_statuses without dropping untouched live entries", () => {
  const withLive = { ...base(), tabStatuses: { "p-2": "Working" as const } };
  const withStatus = { ...snap(5, "w-new"), tab_statuses: { "p-1": "Waiting" as const } };
  const next = reducer(withLive, { type: "SET_STATE", state: withStatus });
  assert(next.tabStatuses["p-1"] === "Waiting", "new snapshot status applied");
  assert(next.tabStatuses["p-2"] === "Working", "prior live-tracked status preserved");
});



test("end-to-end: hook-driven pending_prompt survives reload as attention (not Working)", () => {
  const promptedTab = {
    id: "p-1",
    pty_id: "pty-1",
    title: "shell",
    pending_prompt: { question: "Allow?", options: [{ label: "Yes", value: "1" }] },
  } as unknown as Tab;
  const withPrompt = {
    ...snap(5, "w-new"),
    tabs: { "p-1": promptedTab },
    tab_statuses: { "p-1": "Waiting" as const },
  };
  const next = reducer(base(), { type: "SET_STATE", state: withPrompt });
  assert(next.tabStatuses["p-1"] === "Waiting", "backend-derived Waiting hydrates, not Working");
  assert(Boolean(next.tabs["p-1"].pending_prompt), "pending_prompt itself survives the snapshot merge");
});

test("SET_STATE drops a stale Waiting once pending_prompt clears and no fresh status is reported", () => {
  const withWaiting = {
    ...base(),
    tabs: {
      "p-1": {
        id: "p-1",
        pty_id: null,
        title: "shell",
        pending_prompt: { question: "Allow?", options: [{ label: "Yes", value: "1" }] },
      } as unknown as Tab,
    },
    tabStatuses: { "p-1": "Waiting" as const },
  };
  const cleared = {
    ...snap(5, "w-new"),
    tabs: {
      "p-1": { id: "p-1", pty_id: null, title: "shell" } as unknown as Tab,
    },
    // No tab_statuses entry for p-1 — the tab has no PTY to derive from.
  };
  const next = reducer(withWaiting, { type: "SET_STATE", state: cleared });
  assert(
    next.tabStatuses["p-1"] === undefined,
    "stale Waiting cleared once pending_prompt is gone and no fresh status reports it",
  );
});

test("SET_STATE keeps a genuine tracker-reported Waiting even without pending_prompt", () => {
  const withWaiting = { ...base(), tabStatuses: { "p-1": "Waiting" as const } };
  const stillWaiting = {
    ...snap(5, "w-new"),
    tab_statuses: { "p-1": "Waiting" as const },
  };
  const next = reducer(withWaiting, { type: "SET_STATE", state: stillWaiting });
  assert(next.tabStatuses["p-1"] === "Waiting", "tracker-reaffirmed Waiting is not dropped");
});

test("boot stage events preserve failure reason and clear it on retry and ready", () => {
  const hydrated = {
    ...base(),
    tabs: { "p-1": { id: "p-1", pty_id: null, title: "General", protected: true } as unknown as Tab },
  };
  const starting = reducer(hydrated, {
    type: "UPDATE_BOOT_STAGE",
    tabId: "p-1",
    stage: "pty_starting",
  });
  assert(starting.tabs["p-1"].boot_stage === "pty_starting", "pty_starting recorded");
  assert(starting.tabs["p-1"].boot_failure_reason === null, "retry clears stale reason");

  const failed = reducer(starting, {
    type: "UPDATE_BOOT_STAGE",
    tabId: "p-1",
    stage: "spawn_failed",
    reason: "provider missing",
  });
  assert(failed.tabs["p-1"].boot_stage === "spawn_failed", "spawn_failed recorded");
  assert(failed.tabs["p-1"].boot_failure_reason === "provider missing", "failure reason recorded");

  const retry = reducer(failed, {
    type: "UPDATE_BOOT_STAGE",
    tabId: "p-1",
    stage: "pty_starting",
  });
  assert(retry.tabs["p-1"].boot_stage === "pty_starting", "retry moves back to starting");
  assert(retry.tabs["p-1"].boot_failure_reason === null, "stale failure reason cleared on retry");

  const ready = reducer(retry, {
    type: "UPDATE_BOOT_STAGE",
    tabId: "p-1",
    stage: "agent_ready",
  });
  assert(ready.tabs["p-1"].boot_stage === "agent_ready", "agent_ready recorded");
  assert(ready.tabs["p-1"].boot_failure_reason === null, "ready leaves no stale failure reason");
});

test("late attach hydration carries spawn failure and later snapshot clears it", () => {
  const failedSnapshot = {
    ...snap(7, "w-new"),
    tabs: {
      "p-1": {
        id: "p-1",
        pty_id: null,
        title: "General",
        protected: true,
        boot_stage: "spawn_failed",
        boot_failure_reason: "child exited",
      } as unknown as Tab,
    },
  };
  const hydrated = reducer(base(), { type: "SET_STATE", state: failedSnapshot });
  assert(hydrated.tabs["p-1"].boot_stage === "spawn_failed", "late attach sees failed stage");
  assert(
    hydrated.tabs["p-1"].boot_failure_reason === "child exited",
    "late attach sees failure detail",
  );

  const readySnapshot = {
    ...snap(8, "w-new"),
    tabs: {
      "p-1": {
        id: "p-1",
        pty_id: "pty-1",
        title: "General",
        protected: true,
        boot_stage: "agent_ready",
        boot_failure_reason: null,
      } as unknown as Tab,
    },
  };
  const ready = reducer(hydrated, { type: "SET_STATE", state: readySnapshot });
  assert(ready.tabs["p-1"].boot_stage === "agent_ready", "new snapshot hydrates ready stage");
  assert(ready.tabs["p-1"].boot_failure_reason === null, "new snapshot clears stale failure detail");
});

function tab(id: string): Tab {
  return { id, pty_id: null, title: id } as unknown as Tab;
}

function snapWithFocus(
  generation: number,
  tabIds: string[],
  focused: string | null,
): MultiplexerState {
  const tabs: Record<string, Tab> = {};
  for (const id of tabIds) tabs[id] = tab(id);
  return {
    workspaces: [],
    tabs,
    active_workspace_id: null,
    focused_tab_id: focused,
    generation,
  };
}

test("initial hydrate adopts focused_tab_id naming an existing Tab", () => {
  const next = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  assert(next.focused_tab_id === "p1", "focus adopted from snapshot");
});

test("snapshot naming a missing Tab preserves prior focus", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p1"], "ghost"),
  });
  assert(
    after.focused_tab_id === "p1",
    "stale focus naming a missing Tab does not clobber valid focus",
  );
});

test("prior focused Tab removed from snapshot clears focused_tab_id to null", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p2"], null),
  });
  assert(after.focused_tab_id === null, "ghost focused_tab_id cleared when Tab removed");
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
  assert(after.focused_tab_id === "p1", "null focus does not clobber valid focus");
});

function tabWithTitle(id: string, title: string, titleLevel?: TitleLevel): Tab {
  return {
    id,
    pty_id: null,
    title,
    title_level: titleLevel,
  } as unknown as Tab;
}

test("SET_STATE drops frontend Auto title in favor of backend Default", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: "p1",
      generation: 1,
    },
  });
  const withAuto = reducer(hydrated, {
    type: "SET_TAB_TITLE",
    tabId: "p1",
    title: "codeterm",
    level: TitleLevel.Auto,
  });
  assert(withAuto.tabs.p1.title === "codeterm", "local Auto title applied");
  const after = reducer(withAuto, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 2,
    },
  });
  assert(after.tabs.p1.title === "Tab 3", "backend title wins over frontend Auto on snapshot");
});

test("SET_STATE keeps frontend User title until backend catches up", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: "p1",
      generation: 1,
    },
  });
  const userPinned = reducer(hydrated, {
    type: "SET_TAB_TITLE",
    tabId: "p1",
    title: "My Worker",
    level: TitleLevel.User,
  });
  const after = reducer(userPinned, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 2,
    },
  });
  assert(after.tabs.p1.title === "My Worker", "User rename survives snapshot until backend sync");
});

test("SET_TAB_TITLE rejects lower priority overwrite", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Pinned", TitleLevel.User) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 1,
    },
  });
  const after = reducer(hydrated, {
    type: "SET_TAB_TITLE",
    tabId: "p1",
    title: "osc-title",
    level: TitleLevel.Auto,
  });
  assert(after === hydrated, "Auto cannot overwrite User title");
  assert(after.tabs.p1.title === "Pinned", "User title unchanged");
});

test("SET_STATE yields to a fresh backend Default rename over a stale carried-over Default title", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "stale-cached-name", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 1,
    },
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "fresh-daemon-name", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 2,
    },
  });
  assert(
    after.tabs.p1.title === "fresh-daemon-name",
    "stale cached Default title yields to the backend's fresh Default rename",
  );
});

test("SET_STATE yields to a fresh backend User rename over a title held across a reconnect outage", () => {
  const beforeOutage = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "before-outage", TitleLevel.User) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 1,
    },
  });
  const afterReconnect = reducer(beforeOutage, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "renamed-during-outage", TitleLevel.User) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 2,
    },
  });
  assert(
    afterReconnect.tabs.p1.title === "renamed-during-outage",
    "title held across a reconnect outage yields to the backend's authoritative rename",
  );
});

test("a genuine pending-local User rename survives an interim snapshot, then converges once backend catches up", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 1,
    },
  });
  const userPinned = reducer(hydrated, {
    type: "SET_TAB_TITLE",
    tabId: "p1",
    title: "My Worker",
    level: TitleLevel.User,
  });
  const interim = reducer(userPinned, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 2,
    },
  });
  assert(interim.tabs.p1.title === "My Worker", "pending local rename survives an interim backend snapshot");
  const confirmed = reducer(interim, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "My Worker", TitleLevel.User) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 3,
    },
  });
  assert(confirmed.tabs.p1.title === "My Worker", "title matches once backend catches up");
  const renamedElsewhere = reducer(confirmed, {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Renamed Elsewhere", TitleLevel.User) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 4,
    },
  });
  assert(
    renamedElsewhere.tabs.p1.title === "Renamed Elsewhere",
    "pending flag cleared on confirmation, so a later tie yields to the backend again",
  );
});

test("SET_TAB_TITLE at User level pins title for tab rename round-trip", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: {
      workspaces: [],
      tabs: { p1: tabWithTitle("p1", "Tab 3", TitleLevel.Default) },
      active_workspace_id: null,
      focused_tab_id: null,
      generation: 1,
    },
  });
  const renamed = reducer(hydrated, {
    type: "SET_TAB_TITLE",
    tabId: "p1",
    title: "Renamed Tab",
    level: TitleLevel.User,
  });
  assert(renamed.tabs.p1.title === "Renamed Tab", "User rename applied on Tab");
  assert(renamed.tabs.p1.title_pending_local === true, "rename marked pending until backend sync");
});

test("RESET_GENERATION lets a low post-restart snapshot apply over a high stale generation", () => {
  const highGen = reducer(base(), { type: "SET_STATE", state: snap(500, "w-old") });
  assert(highGen.generation === 500, "client accumulated a high generation");
  const resetted = reducer(highGen, { type: "RESET_GENERATION" });
  assert(resetted.generation === 0, "generation dropped to 0 on boot-changed");
  assert(resetted.active_workspace_id === "w-old", "workspace/Tab structure untouched by the reset itself");
  const after = reducer(resetted, { type: "SET_STATE", state: snap(1, "w-post-restart") });
  assert(after.generation === 1, "low post-restart generation now applies");
  assert(after.active_workspace_id === "w-post-restart", "post-restart structure replaces stale pre-restart structure");
});

test("without RESET_GENERATION a low post-restart snapshot would be rejected (sanity check)", () => {
  const highGen = reducer(base(), { type: "SET_STATE", state: snap(500, "w-old") });
  const after = reducer(highGen, { type: "SET_STATE", state: snap(1, "w-post-restart") });
  assert(after === highGen, "monotonic guard still rejects a low generation outside the boot-changed path");
});

test("CLOSE_TAB removes every tab whose Tab is in the removed cascade", () => {
  const start: ExtendedState = {
    ...base(),
    workspaces: [
      {
        id: "w1",
        name: "W",
        tabs: ["tab-orch", "tab-planner", "tab-worker"],
        active_tab_id: "tab-worker",
        cwd: null,
      } as unknown as Workspace,
    ],
    tabs: {
      "tab-orch": { id: "tab-orch", title: "Orchestrator" } as unknown as Tab,
      "tab-planner": {
        id: "tab-planner",
        title: "Planner",
        parent_tab_id: "tab-orch",
      } as unknown as Tab,
      "tab-worker": {
        id: "tab-worker",
        title: "Worker",
        parent_tab_id: "tab-planner",
      } as unknown as Tab,
    },
    tabStatuses: {
      "tab-orch": "Idle",
      "tab-planner": "Working",
      "tab-worker": "Waiting",
    },
    activeTabIds: { w1: "tab-worker" },
  };

  const after = reducer(start, {
    type: "CLOSE_TAB",
    workspaceId: "w1",
    tabId: "tab-orch",
    removedTabIds: ["tab-orch", "tab-planner", "tab-worker"],
  });
  const ws = after.workspaces.find((w) => w.id === "w1")!;
  assert(ws.tabs.length === 0, `all cascade tabs removed, got ${ws.tabs.length}`);
  assert(ws.active_tab_id === null, "workspace active tab cleared");
  assert(after.activeTabIds.w1 === undefined, "activeTabIds cleared");
  assert(Object.keys(after.tabs).length === 0, "all cascade Tabs removed");
  assert(Object.keys(after.tabStatuses).length === 0, "all cascade statuses removed");
});

test("CLOSE_TAB removes a manager-root subtree", () => {
  const start: ExtendedState = {
    ...base(),
    workspaces: [
      {
        id: "w1",
        name: "W",
        tabs: ["tab-orch", "tab-manager", "tab-worker"],
        active_tab_id: "tab-worker",
        cwd: null,
      } as unknown as Workspace,
    ],
    tabs: {
      "tab-orch": { id: "tab-orch", title: "Orchestrator" } as unknown as Tab,
      "tab-manager": {
        id: "tab-manager",
        title: "Planner",
        role: "Manager",
        parent_tab_id: "tab-orch",
      } as unknown as Tab,
      "tab-worker": {
        id: "tab-worker",
        title: "Worker",
        role: "Worker",
        parent_tab_id: "tab-manager",
      } as unknown as Tab,
    },
    tabStatuses: {
      "tab-manager": "Working",
      "tab-worker": "Waiting",
    },
    activeTabIds: { w1: "tab-worker" },
  };

  const after = reducer(start, {
    type: "CLOSE_TAB",
    workspaceId: "w1",
    tabId: "tab-manager",
    removedTabIds: ["tab-manager", "tab-worker"],
  });
  const ws = after.workspaces.find((w) => w.id === "w1")!;
  assert(ws.tabs.join(",") === "tab-orch", "only orchestrator remains");
  assert(ws.active_tab_id === "tab-orch", "active tab falls back to orchestrator");
  assert(after.activeTabIds.w1 === "tab-orch", "activeTabIds falls back to orchestrator");
  assert(after.tabs["tab-orch"], "orchestrator Tab remains");
  assert(!after.tabs["tab-manager"] && !after.tabs["tab-worker"], "manager subtree Tabs removed");
  assert(Object.keys(after.tabStatuses).length === 0, "manager subtree statuses removed");
});

function snapWithTabs(
  generation: number,
  pairs: Array<[string, string]>,
): MultiplexerState {
  const tabs: Record<string, Tab> = {};
  for (const [, tabId] of pairs) tabs[tabId] = { id: tabId, title: tabId } as unknown as Tab;
  return {
    workspaces: [
      {
        id: "w1",
        name: "W",
        tabs: pairs.map(([, tabId]) => tabId),
        active_tab_id: pairs[0]?.[1] ?? null,
        cwd: null,
      } as unknown as Workspace,
    ],
    tabs,
    active_workspace_id: "w1",
    focused_tab_id: null,
    generation,
  };
}

function tabIds(state: ExtendedState): string {
  return state.workspaces[0]?.tabs.join(",") ?? "";
}

test("SET_STATE masks a tombstoned Tab from an accepted snapshot", () => {
  const start = { ...base(), generation: 5, pendingCloses: ["tab-B"] };
  const next = reducer(start, {
    type: "SET_STATE",
    state: snapWithTabs(6, [["A", "tab-A"], ["B", "tab-B"]]),
  });
  assert(tabIds(next) === "tab-A", "closed tab B masked out of the accepted snapshot");
  assert(!next.tabs["tab-B"], "masked Tab removed from Tabs map");
  assert(next.pendingCloses.join(",") === "tab-B", "tombstone retained while the Tab is reported");
});

test("SET_STATE self-clears a tombstone when the accepted snapshot omits the Tab", () => {
  const start = { ...base(), generation: 5, pendingCloses: ["tab-B"] };
  const next = reducer(start, {
    type: "SET_STATE",
    state: snapWithTabs(6, [["A", "tab-A"]]),
  });
  assert(tabIds(next) === "tab-A", "only the live tab remains");
  assert(next.pendingCloses.length === 0, "tombstone cleared once the accepted snapshot omits the Tab");
});

test("SET_STATE does NOT clear a tombstone on a rejected stale snapshot that omits the Tab", () => {
  const start = { ...base(), generation: 10, pendingCloses: ["tab-B"] };
  const stale = reducer(start, {
    type: "SET_STATE",
    state: snapWithTabs(5, [["A", "tab-A"]]),
  });
  assert(stale === start, "stale snapshot is rejected outright");
  assert(stale.pendingCloses.join(",") === "tab-B", "rejected snapshot must not clear the tombstone");
  const later = reducer(stale, {
    type: "SET_STATE",
    state: snapWithTabs(11, [["A", "tab-A"], ["B", "tab-B"]]),
  });
  assert(tabIds(later) === "tab-A", "surviving tombstone still masks B on a later accepted snapshot");
});

test("no time-based expiry: repeated accepted snapshots listing the Tab keep masking", () => {
  let state: ExtendedState = { ...base(), generation: 0, pendingCloses: ["tab-B"] };
  for (let gen = 1; gen <= 50; gen++) {
    state = reducer(state, {
      type: "SET_STATE",
      state: snapWithTabs(gen, [["A", "tab-A"], ["B", "tab-B"]]),
    });
    assert(tabIds(state) === "tab-A", `gen ${gen}: B stays masked`);
    assert(state.pendingCloses.join(",") === "tab-B", `gen ${gen}: tombstone never self-expires`);
  }
  const omitted = reducer(state, {
    type: "SET_STATE",
    state: snapWithTabs(51, [["A", "tab-A"]]),
  });
  assert(omitted.pendingCloses.length === 0, "only an accepted omitting snapshot clears the tombstone");
});

test("REMOVE_PENDING_CLOSE drops the tombstone so a later snapshot restores the Tab", () => {
  const start = { ...base(), generation: 5, pendingCloses: ["tab-B"] };
  const removed = reducer(start, { type: "REMOVE_PENDING_CLOSE", tabIds: ["tab-B"] });
  assert(removed.pendingCloses.length === 0, "explicit remove drops the tombstone");
  const restored = reducer(removed, {
    type: "SET_STATE",
    state: snapWithTabs(6, [["A", "tab-A"], ["B", "tab-B"]]),
  });
  assert(tabIds(restored) === "tab-A,tab-B", "with no tombstone the authoritative Tab reappears");
});

test("CLOSE_TAB records a tombstone for the removed Tab", () => {
  const start: ExtendedState = {
    ...base(),
    workspaces: [
      { id: "w1", name: "W", tabs: ["tab-A"], active_tab_id: "tab-A", cwd: null } as Workspace,
    ],
    tabs: { "tab-A": { id: "tab-A", title: "A" } as unknown as Tab },
    activeTabIds: { w1: "tab-A" },
  };
  const next = reducer(start, {
    type: "CLOSE_TAB",
    workspaceId: "w1",
    tabId: "tab-A",
    removedTabIds: ["tab-A"],
  });
  assert(next.pendingCloses.join(",") === "tab-A", "closing a tab tombstones its Tab");
});

console.log(failures === 0 ? "✓ reducer tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
