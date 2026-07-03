// Run: npx tsx src/store/multiplexer/reducer.test.ts
// SET_STATE monotonic generation guard (T3 Fix B): a snapshot whose generation

import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Workspace, Pane } from "../../lib/types";
import { TitleLevel } from "../../lib/types";
import { resolveTabStatusRailVariant } from "../../components/Sidebar/TabStatusRail";

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
    active_workspace_id: wsId,
    focused_pane_id: null,
    generation,
  };
}

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

test("end-to-end: a reload snapshot's pane_statuses drives the attention rail", () => {
  // Closes the loop from backend snapshot -> reducer -> rail resolver, so a
  // page reload renders the correct rail without waiting for a live event.
  const withPrompt = {
    ...snap(5, "w-new"),
    pane_statuses: { "p-1": "Waiting" as const },
  };
  const next = reducer(base(), { type: "SET_STATE", state: withPrompt });
  const variant = resolveTabStatusRailVariant(next.paneStatuses["p-1"], false, false);
  assert(variant === "attention", "hydrated Waiting status resolves to the attention rail");
});

test("end-to-end: hook-driven pending_prompt survives reload as attention (not Working)", () => {
  // Regression for the reload race this task fixes: a hook-driven prompt on a
  // still-producing-output pane (e.g. a plain shell loop) has no agent-marker
  // path to ever call PaneStatusTracker::set_waiting, so snapshot_with_status
  // must derive Waiting from pane.pending_prompt itself — mirrored here as the
  // backend now would send it, NOT the raw Working the old tracker mirror gave.
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
  const variant = resolveTabStatusRailVariant(
    next.paneStatuses["p-1"],
    Boolean(next.panes["p-1"].pending_prompt),
    false,
  );
  assert(variant === "attention", "hook-driven prompt resolves to attention after reload, not working");
});

test("SET_STATE drops a stale Waiting once pending_prompt clears and no fresh status is reported", () => {
  // Regression: a pane with no attached PTY (pty_id null) has nothing for the
  // backend's snapshot_with_status to report once its hook-driven
  // pending_prompt is cleared — the key is simply absent from every future
  // pane_statuses map. A pure merge would leave "Waiting" stuck on this pane
  // forever; the reducer must drop it once the pane's own pending_prompt is
  // gone and the fresh snapshot doesn't re-affirm any status for it.
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
  // A REAL PaneStatusTracker-detected permission prompt (agent marker path,
  // not the hook-explicit path) can set Waiting with no pending_prompt on the
  // pane at all — that must survive since the fresh snapshot re-affirms it.
  const withWaiting = { ...base(), paneStatuses: { "p-1": "Waiting" as const } };
  const stillWaiting = {
    ...snap(5, "w-new"),
    pane_statuses: { "p-1": "Waiting" as const },
  };
  const next = reducer(withWaiting, { type: "SET_STATE", state: stillWaiting });
  assert(next.paneStatuses["p-1"] === "Waiting", "tracker-reaffirmed Waiting is not dropped");
});

// SET_STATE focused_pane_id: under the payload-based mux-state-changed flow,
// snapshots now carry focused_pane_id. The reducer must adopt it when it names
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
  // New snapshot no longer contains p1 (closed/rolled back), incoming focus null
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
  // Simulates a cache-hydrated (or previously-synced) pane whose title never
  // went through a local SET_PANE_TITLE dispatch — title_pending_local is
  // unset, so a level tie must resolve to the backend's authoritative title.
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
  // Same tie (User vs User) as a genuine pending local rename, but here BOTH
  // titles arrived via SET_STATE (e.g. the second rename happened on the
  // daemon while this client was disconnected) — neither is locally pending,
  // so the daemon's later rename must win, not whichever the client saw first.
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

test("RESET_GENERATION lets a low post-restart snapshot apply over a high stale generation", () => {
  // Regression for the boot-changed / daemon-restart structure-resync bug: a
  // long-lived web client accumulates a high generation, the daemon restarts
  // and its own counter resets to a low value, so without RESET_GENERATION
  // the monotonic guard would reject every post-restart snapshot forever.
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

console.log(failures === 0 ? "✓ reducer tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
