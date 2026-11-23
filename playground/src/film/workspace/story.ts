import type { MachineIdentity } from "../../../../src/lib/machineIdentity";
import { RELAY_PROTOCOL_VERSION } from "../../../../src/lib/types";
import type { FileDiff, NormalizedChatMessage } from "@codeterm/chat-core";
import type { PlanIndexEntry } from "../../../../src/components/Canvas/plansApi";
import type { Plan, Task, TaskLaneMetadata } from "../../../../src/components/Canvas/types";
import type { Tab, Team, Workspace } from "../../../../src/lib/types";
import { CODEX, CURSOR, OPENCODE } from "../../gallery/fixtures";
import { CHAT_CARDS } from "../fixtures/chat";
import { ago, FILM_NOW } from "../fixtures/clock";
import { PLAN as FILM_PLAN, SECTIONS } from "../fixtures/plan";
import { MEMBERS, TEAM } from "../fixtures/team";
import { compileBeat, type Beat, type Step } from "./beat";
import type { Activity } from "./activity";

/**
 * One storyline across every place (spec L13.4): Atlas splits the 2.4 gate into four tasks, Mira takes the resolver on
 * this computer and Orbit runs sync QA on gpu-rig, Mira fails a test, fixes it and reports, and the board advances.
 * Every beat runs on the same story clock, so a report arrives in Helm's chat the moment its sender finishes.
 */

const [HELM_TAB, ATLAS_TAB, LEDGER_TAB, MIRA_TAB, ORBIT_TAB, QUILL_TAB] = MEMBERS;
export const PLAN_ID = FILM_PLAN.id;

export type MachineId = "local" | "laptop" | "gpu-rig" | "studio";

export interface Machine {
  id: MachineId;
  name: string;
  workspaces: Workspace[];
  tabs: Tab[];
  teams: Team[];
  /** The tab a visitor lands on when switching to this machine. */
  home: string;
}

const idle = (tab: Tab, line = "Churned for 4m 12s"): Tab => ({ ...tab, agent_state: { state: "idle", status_line: line } });

// ---- Tabs ----

export const HELM = idle(HELM_TAB);
export const ATLAS = idle({ ...ATLAS_TAB, agent_type: OPENCODE.id, agent_session_model: "opencode/gemini-3-flash" });
export const LEDGER = idle({ ...LEDGER_TAB, workspace_id: "ws-site", team_id: null });
export const MIRA = idle({ ...MIRA_TAB, agent_type: CODEX.id, agent_session_model: "gpt-6-sol" });
export const QUILL = idle({ ...QUILL_TAB, agent_type: CURSOR.id, agent_session_model: "composer-2.5" });
const WREN = idle({
  ...QUILL_TAB, id: "demo-release-qa", title: "Wren", role_profile: "verification",
  agent_type: OPENCODE.id, agent_session_model: "deepseek/deepseek-v4-pro",
  agent_session_id: "session-wren", pty_id: "pty-wren",
});
// Orbit lives on gpu-rig: a mesh Worker reports through the mesh, it is not a member of this computer's Team.
export const ORBIT = idle({ ...ORBIT_TAB, team_id: null, parent_tab_id: null, workspace_id: "ws-rig", cwd: "/home/build/fieldnote" });

const MEDIA_ID = "film-media-team";
const SITE_WS = "ws-site";
export const MEDIA_TEAM: Team = { id: MEDIA_ID, name: "Media team", lifecycle: "active", created_at: ago(9 * 24 * 60), workspace_id: TEAM.workspace_id, remote_peer_id: "peer-gpu-rig", remote_peer_name: "gpu-rig" };

function mediaMember(id: string, title: string, role: Tab["role"], profile: string, model: string, parent: string | null, agent = HELM_TAB.agent_type): Tab {
  return idle({
    ...HELM_TAB, id, agent_type: agent, title, role, role_profile: profile, parent_tab_id: parent, team_id: MEDIA_ID, workspace_id: TEAM.workspace_id, cwd: "C:/Developer/fieldnote",
    pty_id: `pty-${id}`, agent_session_id: `session-${id}`, agent_session_model: model, can_spawn: profile === "orchestrator",
  }, "Idle for 18m");
}

// The Media team's package roles: a producer leads with a design lead and an engineering lead; writer and analyst work.
// Lead kinds are spelled with a space so the role badge reads "Design lead", as the app will once task_qy7ofz lands.
const PRODUCER = "b81f4c22";
export const MEDIA: Tab[] = [
  mediaMember(PRODUCER, "Lark", "Manager", "orchestrator", "opus", null),
  mediaMember("c4a09e71", "Moss", "Manager", "design lead", "opencode/gemini-3-flash", PRODUCER, OPENCODE.id),
  mediaMember("d3e7b518", "Reed", "Manager", "engineering lead", "gpt-6.1-sol", PRODUCER, CODEX.id),
  mediaMember("e92c6d04", "Fern", "Worker", "writer", "deepseek/deepseek-v4-pro", PRODUCER, OPENCODE.id),
  mediaMember("f1b5a83e", "Cove", "Worker", "analyst", "composer-2.5", PRODUCER, CURSOR.id),
];

const LAPTOP_TAB: Tab = idle({ ...QUILL_TAB, id: "3a6d0f9c", title: "Sable", role: "Worker", role_profile: null, parent_tab_id: null, team_id: null, workspace_id: "ws-binaura", cwd: "/Users/me/Developer/binaura", agent_type: OPENCODE.id, agent_session_model: "opencode/qwen3.6-plus", pty_id: "pty-sable", agent_session_id: "session-sable" }, "Idle for 2h");

const STUDIO_TAB = idle({
  ...ORBIT, agent_type: OPENCODE.id, agent_session_model: "opencode/kimi-k2.7-code", id: "demo-studio-agent", title: "Rowan", workspace_id: "ws-studio",
  cwd: "/home/demo/fieldnote", agent_session_id: "session-studio", pty_id: "pty-studio",
});

const LOCAL_FIELDNOTE = [HELM, ATLAS, MIRA, QUILL, WREN];

export const MACHINES: Record<MachineId, Machine> = {
  local: {
    id: "local",
    name: "This computer",
    workspaces: [
      { id: TEAM.workspace_id!, name: "Fieldnote", tabs: [...LOCAL_FIELDNOTE, ...MEDIA].map((tab) => tab.id), active_tab_id: HELM.id, cwd: "C:/Developer/fieldnote" },
      { id: SITE_WS, name: "Fieldnote site", tabs: [LEDGER.id], active_tab_id: LEDGER.id, cwd: "C:/Developer/fieldnote-site" },
    ],
    tabs: [...LOCAL_FIELDNOTE, ...MEDIA, LEDGER],
    teams: [TEAM, MEDIA_TEAM],
    home: HELM.id,
  },
  laptop: {
    id: "laptop",
    name: "laptop",
    workspaces: [{ id: "ws-binaura", name: "Fieldnote · laptop", tabs: [LAPTOP_TAB.id], active_tab_id: LAPTOP_TAB.id, cwd: "/Users/me/Developer/binaura" }],
    tabs: [LAPTOP_TAB],
    teams: [],
    home: LAPTOP_TAB.id,
  },
  "gpu-rig": {
    id: "gpu-rig",
    name: "gpu-rig",
    workspaces: [{ id: "ws-rig", name: "Fieldnote", tabs: [ORBIT.id], active_tab_id: ORBIT.id, cwd: "/home/build/fieldnote" }],
    tabs: [ORBIT],
    teams: [],
    home: ORBIT.id,
  },
  studio: {
    id: "studio", name: "studio",
    workspaces: [{ id: "ws-studio", name: "Fieldnote · studio", tabs: [STUDIO_TAB.id], active_tab_id: STUDIO_TAB.id, cwd: STUDIO_TAB.cwd }],
    tabs: [STUDIO_TAB], teams: [], home: STUDIO_TAB.id,
  },
};

export const MEDIA_HOME = PRODUCER;

/** The mesh as the picker lists it from this computer. */
const peer = (id: string, name: string, ip: string, activeAgo: number) => ({
  instance_id: id, instance_name: name, addresses: [{ ip, is_tailscale: true }], api_port: 7685, public_key: "", paired_at: new Date(ago(40 * 24 * 60)).toISOString(),
  last_seen: new Date(FILM_NOW - activeAgo).toISOString(), online: true, is_primary: false, access_level: "full", version: "1.20.1",
  relay_protocol_version: RELAY_PROTOCOL_VERSION, last_active_ms: FILM_NOW - activeAgo,
});

export const PEERS = {
  local_instance_name: MACHINES.local.name,
  paired: [peer("peer-gpu-rig", "gpu-rig", "100.86.12.40", 40_000), peer("peer-laptop", "laptop", "100.86.12.17", 2 * 60 * 60_000)],
  // Sample machine identities, so the 1.22 switcher and Team pills show each machine's glyph and tone.
  local_machine_identity: { kind: "desktop", color: "blue" } as MachineIdentity,
  machine_identities: {
    "peer-gpu-rig": { kind: "server", color: "teal" },
    "peer-laptop": { kind: "laptop", color: "violet" },
    "peer-studio": { kind: "mac", color: "yellow" },
  } as Record<string, MachineIdentity>,
};

export const PEER_MACHINE: Record<string, MachineId> = { "peer-gpu-rig": "gpu-rig", "peer-laptop": "laptop", "peer-studio": "studio" };

// ---- Plan ----

const CANVAS = { x: null, y: null, collapsed: false };

function lane(tabId: string, slug: string): TaskLaneMetadata {
  return { tab_id: tabId, branch: `lane/offline-sync-${slug}`, worktree_path: `C:/Developer/fieldnote-worktrees/offline-sync-${slug}`, workspace_name: "Fieldnote" };
}

function task(id: string, key: string, name: string, status: Task["status"], overrides: Partial<Task> = {}): Task {
  return {
    id, key, name, type: "implement", model: null, model_rationale: null, status, status_label: null, section_ids: [], body: "",
    intent: null, parent_task_id: null, assigned_tab_id: null, peer: null, canvas: CANVAS, milestones: [], ...overrides,
  };
}

const RESOLVER_FROM = FILM_PLAN.tasks.find((t) => t.id === "task-resolver")!;

export const T = { resolver: "task_fn203r", qa: "task_fn204q", guide: "task_fn206g", background: "task_fn207b" } as const;

const PRIOR: Task[] = [
  task("task_fn201s", "FN-201", "Schema migration v7 to v8: vector clocks and tombstones", "done", { section_ids: ["sec-store"], assigned_tab_id: MIRA.id, lane: lane(MIRA.id, "schema") }),
  task("task_fn202o", "FN-202", "Offline edit queue with ordered replay", "done", { section_ids: ["sec-sync"], assigned_tab_id: MIRA.id, lane: lane(MIRA.id, "queue") }),
  task("task_fn205b", "FN-205", "Sync status badge in the editor", "done", { section_ids: ["sec-sync"], assigned_tab_id: QUILL.id, lane: lane(QUILL.id, "badge") }),
];

const SPLIT: Task[] = [
  task(T.resolver, "FN-203", RESOLVER_FROM.name, "pending", {
    priority: "p1", section_ids: ["sec-sync"], executor_role: RESOLVER_FROM.executor_role, role_guidance: RESOLVER_FROM.role_guidance,
    body: RESOLVER_FROM.body, acceptance_criteria: RESOLVER_FROM.acceptance_criteria,
  }),
  task(T.qa, "FN-204", "Sync QA matrix on gpu-rig", "pending", {
    section_ids: ["sec-qa"], body: "Run the 48-scenario matrix: airplane mode, clock skew, three devices editing one note, delete versus edit.",
    acceptance_criteria: ["48 of 48 scenarios pass", "0 divergent replicas after the soak"],
  }),
  task(T.guide, "FN-206", "Offline sync guide and migration notes", "pending", { section_ids: ["sec-docs"] }),
  task(T.background, "FN-207", "Background sync on iOS and Android", "pending", { section_ids: ["sec-sync"], body: "After 2.4: sync while the app is in the background." }),
];

// ---- Beats: each plays on its own when the visitor opens its tab or a chip replays it ----

const tool = (id: string, toolKind: NonNullable<NormalizedChatMessage["toolKind"]>, toolName: string, toolInput: Record<string, unknown>, toolSummary: string, extra: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({
  id, type: "tool_call", content: "", provider: "claude", toolName, toolId: `${id}-call`, toolKind, toolInput, toolArgs: JSON.stringify(toolInput), toolSummary, ...extra,
});

const RESOLVER_OLD = `export function resolve(base: Note, local: Note, remote: Note): Note {
  if (remote.deletedAt) return remote;
  return mergeFields(base, local, remote);
}`;
const RESOLVER_TRY = `export function resolve(base: Note, local: Note, remote: Note): Note {
  if (remote.deletedAt && !edited(base, local)) return remote;
  return mergeFields(base, local, remote);
}`;
const RESOLVER_FIX = `export function resolve(base: Note, local: Note, remote: Note): Note {
  if (remote.deletedAt && !edited(base, local)) return remote;
  const merged = mergeFields(base, local, remote);
  // An edit on another device brings a deleted note back with that edit.
  return remote.deletedAt ? { ...merged, deletedAt: null } : merged;
}`;
const RESOLVER_PATH = "packages/sync/src/resolver.ts";

// The provider's own edit record: one hunk per edit, with context lines, as the Edit card draws it.
const diff = (newStart: number, lines: string[]): FileDiff[] => {
  const additions = lines.filter((l) => l.startsWith("+")).length;
  const deletions = lines.filter((l) => l.startsWith("-")).length;
  const context = lines.length - additions - deletions;
  return [{ path: RESOLVER_PATH, kind: "modify", additions, deletions, hunks: [{ oldStart: newStart, oldLines: context + deletions, newStart, newLines: context + additions, lines }] }];
};
const FIRST_DIFF = diff(12, [
  " import { edited, mergeFields } from \"./merge\";",
  " ",
  " export function resolve(base: Note, local: Note, remote: Note): Note {",
  "-  if (remote.deletedAt) return remote;",
  "+  if (remote.deletedAt && !edited(base, local)) return remote;",
  "   return mergeFields(base, local, remote);",
  " }",
]);
const FIX_DIFF = diff(14, [
  " export function resolve(base: Note, local: Note, remote: Note): Note {",
  "   if (remote.deletedAt && !edited(base, local)) return remote;",
  "-  return mergeFields(base, local, remote);",
  "+  const merged = mergeFields(base, local, remote);",
  "+  // An edit on another device brings a deleted note back with that edit.",
  "+  return remote.deletedAt ? { ...merged, deletedAt: null } : merged;",
  " }",
]);

const MIRA_STEPS: Step[] = [
  { kind: "say", id: "mira-1", text: "Taking FN-203. Reading the resolver first." },
  { kind: "tool", call: tool("mira-read", "read", "Read", { file_path: RESOLVER_PATH }, RESOLVER_PATH), result: RESOLVER_OLD, seconds: 0.9 },
  { kind: "tool", call: tool("mira-edit-1", "edit", "Edit", { file_path: RESOLVER_PATH }, RESOLVER_PATH, { toolEdits: [{ path: RESOLVER_PATH, old: RESOLVER_OLD, new: RESOLVER_TRY }], fileDiffs: FIRST_DIFF }), result: "The file has been updated.", seconds: 0.7 },
  { kind: "tool", call: tool("mira-test-1", "command", "Bash", { command: "npx vitest run packages/sync" }, "npx vitest run packages/sync"), seconds: 2.4, error: true, exitCode: 1,
    result: " FAIL  test/resolver.test.ts > delete vs edit > restores the note with the edit\nAssertionError: expected undefined to be 'Buy oat milk'\n\n Test Files  1 failed | 11 passed (12)\n      Tests  1 failed | 213 passed (214)\n   Duration  2.38s" },
  { kind: "think", seconds: 0.8 },
  { kind: "say", id: "mira-2", text: "One failure. Actually, the tombstone still wins after the merge: a note deleted on one device stays deleted even though another device edited it. The edit has to bring it back." },
  { kind: "tool", call: tool("mira-edit-2", "edit", "Edit", { file_path: RESOLVER_PATH }, RESOLVER_PATH, { toolEdits: [{ path: RESOLVER_PATH, old: RESOLVER_TRY, new: RESOLVER_FIX }], fileDiffs: FIX_DIFF }), result: "The file has been updated.", seconds: 0.8 },
  { kind: "tool", call: tool("mira-test-2", "command", "Bash", { command: "npx vitest run packages/sync" }, "npx vitest run packages/sync"), seconds: 2.3, exitCode: 0,
    result: " ✓ test/resolver.test.ts (214 tests) 1.91s\n\n Test Files  12 passed (12)\n      Tests  214 passed (214)\n   Duration  2.27s" },
  { kind: "tool", call: tool("mira-commit", "command", "Bash", { command: "git commit -am \"sync: an edit restores a note deleted elsewhere\"" }, "git commit -am \"sync: an edit restores a note deleted elsewhere\""), seconds: 0.7, exitCode: 0,
    result: "[lane/offline-sync-resolver 8c41e2f] sync: an edit restores a note deleted elsewhere\n 2 files changed, 18 insertions(+), 6 deletions(-)" },
  { kind: "say", id: "mira-3", text: "Done. Reported to Helm: in review at `8c41e2f`, 214 passed, +18 −6 in 2 files." },
];

const ORBIT_STEPS: Step[] = [
  { kind: "say", id: "orbit-1", text: "Starting the sync QA matrix on gpu-rig: 48 scenarios, three replicas each." },
  { kind: "tool", call: tool("orbit-pull", "command", "exec", { cmd: "git pull --ff-only" }, "git pull --ff-only", { provider: "codex" }), seconds: 0.9, exitCode: 0, result: "Updating 3e7b0d2..5b91c4a\nFast-forward\n 4 files changed, 61 insertions(+), 12 deletions(-)" },
  { kind: "tool", call: tool("orbit-qa", "command", "exec", { cmd: "npm run qa:sync -- --matrix all" }, "npm run qa:sync -- --matrix all", { provider: "codex" }), seconds: 2.5, exitCode: 0,
    result: "airplane-mode    12/12 ✓\nclock-skew       12/12 ✓\nthree-devices    16/16 ✓\ndelete-vs-edit    8/8  ✓\n\n48 of 48 scenarios passed · 0 divergent replicas" },
  { kind: "tool", call: tool("orbit-read", "read", "read", { file_path: "qa/results/gpu-rig.json" }, "qa/results/gpu-rig.json", { provider: "codex" }), seconds: 0.6, result: "{ \"passed\": 48, \"failed\": 0, \"divergent\": 0, \"machine\": \"gpu-rig\" }" },
  { kind: "say", id: "orbit-2", text: "Sent the result to Helm over the mesh. Results stay cached here for the gate." },
];

export const MIRA_BEAT = compileBeat(
  [{ id: "mira-ask", type: "user", inboundKind: "tab", fromLabel: "Helm", fromTab: HELM.id, fromModel: "opus", content: `Take ${T.resolver} in ${PLAN_ID}: conflict resolver for concurrent edits. Work on lane/offline-sync-resolver and report when it is in review.` }],
  MIRA_STEPS.map((step) => step.kind === "tool" ? { ...step, call: { ...step.call, provider: "codex" } } : step), 3,
);

export const ORBIT_BEAT = compileBeat(
  [{ id: "orbit-ask", type: "user", inboundKind: "mesh", fromLabel: "Helm", fromTab: HELM.id, fromModel: "opus", fromMesh: MACHINES.local.name, content: `Run ${T.qa} in ${PLAN_ID}: the sync QA matrix on gpu-rig. Report the result to me.` }],
  ORBIT_STEPS, 5,
);

const MIRA_REPORT: NormalizedChatMessage = {
  id: "helm-mira-report", type: "user", inboundKind: "report", fromLabel: "Mira", fromTab: MIRA.id, fromModel: MIRA.agent_session_model ?? undefined,
  content: `STATUS: in_review\nCHANGES: conflict resolver on lane/offline-sync-resolver at 8c41e2f (+18 −6, 2 files)\nTESTS RUN: vitest 214 passed, 0 failed; the first run failed delete-vs-edit, fixed in resolver.ts\nFOLLOWUP: Orchestrator reviews and merges ${T.resolver} in ${PLAN_ID}`,
};

const ORBIT_REPORT: NormalizedChatMessage = {
  id: "helm-orbit-mesh", type: "user", inboundKind: "mesh", fromLabel: "Orbit", fromTab: ORBIT.id, fromModel: "codex", fromMesh: "gpu-rig",
  content: "Sync QA green on gpu-rig: 48 of 48 scenarios, 0 divergent replicas. Results are cached for the gate.",
};

const ATLAS_SPLIT: NormalizedChatMessage = {
  id: "helm-atlas-split", type: "user", inboundKind: "tab", fromLabel: "Atlas", fromTab: ATLAS.id, fromModel: ATLAS.agent_session_model ?? undefined,
  content: `Split the 2.4 gate into 4 tasks in ${PLAN_ID}: ${T.resolver} conflict resolver for Mira, ${T.qa} sync QA matrix for Orbit on gpu-rig, ${T.guide} offline sync guide for Quill, and ${T.background} background sync after 2.4.`,
};

export const DRAFT = "Run the 2.4 release gate and get sync QA green on gpu-rig.";

const HELM_STEPS: Step[] = [
  { kind: "type", id: "helm-ask", text: DRAFT },
  { kind: "think", seconds: 0.9 },
  { kind: "tool", call: tool("helm-plan", "command", "Bash", { command: `domios plan show ${PLAN_ID}` }, `domios plan show ${PLAN_ID}`), seconds: 1.1, exitCode: 0,
    result: "Offline sync · active · 3 done\nGoal: Fieldnote works fully offline; edits made on any device sync later and never overwrite each other." },
  { kind: "say", id: "helm-1", text: "Asking Atlas to split the gate into tasks." },
  { kind: "think", seconds: 1.2 },
  { kind: "arrive", message: ATLAS_SPLIT },
  { kind: "say", id: "helm-2", text: "On it. **Mira** takes the resolver here, **Orbit** runs sync QA on gpu-rig. I'll post the result here." },
  { kind: "think", seconds: 3.4 },
  { kind: "arrive", message: ORBIT_REPORT },
  // The mesh report is the beat the visitor came for; nothing streams over it for a while.
  { kind: "think", seconds: 2.2 },
  { kind: "arrive", message: MIRA_REPORT },
  { kind: "think", seconds: 0.9 },
  { kind: "tool", call: tool("helm-diff", "command", "Bash", { command: "git diff --stat release/2.4...lane/offline-sync-resolver" }, "git diff --stat release/2.4...lane/offline-sync-resolver"), seconds: 1.2, exitCode: 0,
    result: " packages/sync/src/resolver.ts             |  9 ++++++---\n packages/sync/test/resolver.test.ts       | 15 +++++++++++----\n 2 files changed, 18 insertions(+), 6 deletions(-)" },
  { kind: "say", id: "helm-3", text: "Both are in. QA is 48 of 48 on gpu-rig, and Mira's fix is small: the edit now restores a note deleted on another device. Reviewing the diff, then I merge FN-203." },
];

export const HELM_BEAT = compileBeat(
  [CHAT_CARDS[0].message, { id: "helm-morning", type: "assistant", content: "Morning. The overnight soak is green and yesterday's lanes merged clean. The resolver is next." }],
  HELM_STEPS, 1,
);

export const HELM_ORIGINS: Record<string, (typeof CHAT_CARDS)[number]["origin"]> = { [CHAT_CARDS[0].message.id]: CHAT_CARDS[0].origin };

const still = (history: NormalizedChatMessage[]): Beat => compileBeat(history, [], 0, -1);

export const BEATS: Record<string, Beat> = {
  [HELM.id]: HELM_BEAT,
  [MIRA.id]: MIRA_BEAT,
  [ORBIT.id]: ORBIT_BEAT,
  [ATLAS.id]: still([
    { id: "atlas-ask", type: "user", inboundKind: "tab", fromLabel: "Helm", fromTab: HELM.id, fromModel: "opus", content: `Split the 2.4 release gate into tasks in ${PLAN_ID}.` },
    { id: "atlas-reply", type: "assistant", content: `Four tasks, one owner each. The resolver and QA can run in parallel; the guide waits for the resolver's behaviour to settle.\n\n- ${T.resolver} Mira\n- ${T.qa} Orbit on gpu-rig\n- ${T.guide} Quill\n- ${T.background} after 2.4` },
  ]),
  [LEDGER.id]: still([{ id: "ledger-1", type: "assistant", content: "Recorded yesterday's strides: FN-201 and FN-202 merged with their proof, FN-205 shipped behind the badge flag." }]),
  [WREN.id]: still([{ id: "wren-1", type: "assistant", content: "I have the regression fixtures ready for an independent check." }]),
  [QUILL.id]: still([{ id: "quill-1", type: "assistant", content: "Guide outline is up: Working offline, How conflicts merge, Migrating from 2.3. Waiting on FN-203 for the conflict section." }]),
  [STUDIO_TAB.id]: still([{ id: "studio-ready", type: "assistant", content: "Studio joined the mesh. The release branch is available for an independent check." }]),
  [LAPTOP_TAB.id]: still([{ id: "sable-1", type: "assistant", content: "The laptop has a clean copy of Fieldnote and a local set of sync fixtures." }]),
  [MEDIA[0].id]: still([
    { id: "juno-1", type: "user", content: "Plan next week's posts around the offline sync release." },
    { id: "juno-2", type: "assistant", content: "Drafting the week: one launch post, a 40 s clip from the real sync UI, two threads. Moss owns the cover, Fern the copy, Cove the baseline numbers." },
  ]),
  ...Object.fromEntries(MEDIA.slice(1).map((tab) => [tab.id, still([{ id: `${tab.id}-1`, type: "assistant", content: `Waiting for Lark's plan for the offline sync week.` }])])),
};

/** Status line a tab shows in the sidebar while its beat runs. */
export const WORKING_LINE: Record<string, string> = {
  [HELM.id]: "Coordinating the 2.4 gate…",
  [MIRA.id]: "Fixing the conflict resolver…",
  [ORBIT.id]: "Sync QA matrix on gpu-rig…",
};

export const ACTIVITY_PERIOD = 63;

const SCENARIOS = [
  {
    title: "Delete versus edit", check: "delete-edit",
    request: "Can we ship offline edits without losing notes?",
    assignment: "A note vanished when two devices rejoined. Which order should the regression cover?",
    plan: "Delete on one device, edit on another, reconnect the editor last. The edit must restore the note.",
    rig: "48 cases passed. A third device gets the restored note and no duplicate.",
    laptop: "Airplane-mode repro passed. The note returns as soon as the edit syncs.",
    review: "The regression now checks the note body as well as its presence.",
    guide: "The guide now shows how an offline edit can restore a deleted note.",
    caption: "I replaced the before/after stills with the same note ID so readers can follow the restore.",
    media: "Lead with the recovered note. The two-device example explains why we ran the test.",
    close: "The two machines agree, and the regression covers the note body. Marking this case done.",
  },
  {
    title: "Clock skew", check: "clock-skew",
    request: "The laptop clock is six hours ahead. Does sync still converge?",
    assignment: "The later wall-clock timestamp wins in this log. Can we isolate the order from the time on the devices?",
    plan: "Replay the same operations with different local clocks. Compare operation order and final state.",
    rig: "All replicas converged with clocks skewed by six hours in both directions.",
    laptop: "The laptop showed the correct note after reconnecting. Its local time stayed six hours ahead.",
    review: "The fixture holds operation order constant while changing timestamps. It catches the old comparator.",
    guide: "Troubleshooting now explains why adjusting the local clock does not change what sync merges.",
    caption: "The timeline labels now show device order so they can’t be mistaken for wall-clock times.",
    media: "Keep this in troubleshooting. The example is clearer with two clocks side by side.",
    close: "The traces confirm it: changing the time on one device leaves the merged note unchanged.",
  },
  {
    title: "Reconnect order", check: "reconnect-order",
    request: "One more check before the release: reconnect the devices in reverse order.",
    assignment: "The queue looks clean on the first device. Where should we check for drops when the last device reconnects?",
    plan: "Check the pending note on each device and compare the final note count after the last reconnect.",
    rig: "All replicas converged in reverse order. The queued note appeared exactly once on each device.",
    laptop: "The laptop kept its draft through both reconnects. The queued note appeared after the last device came back.",
    review: "I added an assertion for the queue length at the last reconnect. It’s zero once every device catches up.",
    guide: "The walkthrough now shows the offline badge turning off after the final reconnect.",
    caption: "I cut the extra join animation and kept the final queue count visible in the clip.",
    media: "Use that final shot. It gives viewers time to see the badge clear.",
    close: "Every device has the note and the queue drained. Reverse-order reconnect is clear for release.",
  },
];

function incoming(id: string, from: Tab, content: string, mesh?: string, report = false): Step {
  return { kind: "arrive", message: {
    id, type: "user", content, fromLabel: from.title, fromTab: from.id,
    fromModel: from.agent_session_model ?? undefined, fromMesh: mesh,
    inboundKind: mesh ? "mesh" : report ? "report" : "tab",
  } };
}

function script(scenario: typeof SCENARIOS[number], seed: number): Activity {
  const steps: Record<string, Step[]> = {};
  const at = (tab: Tab, seconds: number, step: Step) =>
    (steps[tab.id] ??= []).push({ kind: "until", at: seconds }, step);
  const say = (tab: Tab, seconds: number, id: string, text: string) => at(tab, seconds, { kind: "say", id, text });
  const run = (tab: Tab, seconds: number, id: string, command: string, result: string) =>
    at(tab, seconds, {
      kind: "tool", call: tool(id, "command", "exec", { cmd: command }, command, { provider: tab.agent_type ?? undefined }),
      result, seconds: 2.8, exitCode: 0,
    });
  const { check, request, assignment, plan, rig, laptop, review, guide, caption, media, close } = scenario;
  const [lark, moss, reed, fern, cove] = MEDIA;

  at(HELM, 2.3, { kind: "type", id: "request", text: request });
  at(ATLAS, 7, incoming("plan-request", HELM, assignment));
  const spec = `qa/fixtures/${check}.json`;
  at(ATLAS, 9.6, {
    kind: "tool", call: tool("fixture", "read", "Read", { file_path: spec }, spec, { provider: OPENCODE.id }),
    seconds: 2.1, result: `Case: ${scenario.title}\nAssertions: pending operations, final note state, replica agreement`,
  });
  say(ATLAS, 12.1, "plan", plan);
  if (seed === 0) {
    run(HELM, 14.9, "board", `domios plan show ${PLAN_ID}`, "Release 2.4 · Verification in progress · gpu-rig and laptop assigned");
  } else {
    say(HELM, 14.9, "handoff", `Orbit has the ${scenario.title.toLowerCase()} fixture on gpu-rig. Sable will check the laptop UI.`);
  }
  run(ORBIT, 17, "matrix", `npm run qa:sync -- --case ${check}`, rig);
  say(ORBIT, 21.1, "matrix-result", rig);
  say(moss, 24, "caption", caption);
  say(LAPTOP_TAB, 27, "laptop-result", laptop);
  at(HELM, 30.4, seed % 2 === 0 ? incoming("rig-report", ORBIT, rig, "gpu-rig") : incoming("laptop-report", LAPTOP_TAB, laptop, "laptop"));
  run(WREN, 33.8, "regression", `npx vitest run qa/sync.test.ts -t ${check}`, review);
  say(MIRA, 37.7, "review", review);
  say(reed, 40.5, "evidence", `I added the ${check} repro and the QA fixture link under the clip.`);
  say(QUILL, 43.4, "guide", guide);
  say(fern, 46, "release-note", `Release note draft: ${guide}`);
  say(cove, 48.6, "baseline", `The ${scenario.title.toLowerCase()} draft matches QA. I’ve kept the original fixture beside the result.`);
  at(lark, 51.4, incoming("cover", moss, caption));
  say(lark, 54.2, "publish-note", media);
  say(HELM, 57.8, "decision", close);

  return {
    duration: ACTIVITY_PERIOD,
    beats: Object.fromEntries(Object.entries(steps).map(([id, entries]) => [id, compileBeat([], entries, seed)])),
  };
}

export const ACTIVITY = SCENARIOS.map(script);

const WORK_WINDOWS: [Tab, number, number, string][] = [
  [ATLAS, 6, 15, "Reading the verification fixture…"],
  [ORBIT, 14, 23, "Running the sync matrix…"],
  [LAPTOP_TAB, 16, 29, "Reproducing on laptop…"],
  [WREN, 30, 38, "Running the regression test…"],
  [MIRA, 32, 40, "Reviewing regression coverage…"],
  [QUILL, 35, 46, "Updating the offline guide…"],
  [MEDIA[1], 17, 27, "Editing the release screenshot…"],
  [MEDIA[2], 30, 43, "Checking release evidence…"],
  [MEDIA[3], 39, 49, "Drafting the release note…"],
  [MEDIA[4], 42, 51, "Checking the baseline…"],
  [MEDIA[0], 50, 57, "Reviewing release copy…"],
];

export function workAt(tab: Tab, seconds: number): string | undefined {
  const phase = seconds % ACTIVITY_PERIOD;
  return WORK_WINDOWS.find(([agent, from, to]) => agent.id === tab.id && phase >= from && phase < to)?.[3];
}

export function activePlan(seconds: number, openTaskId?: string): Plan {
  const run = Number.isFinite(seconds) ? Math.floor(seconds / ACTIVITY_PERIOD) : 0;
  const elapsed = Number.isFinite(seconds) ? seconds % ACTIVITY_PERIOD : ACTIVITY_PERIOD - 1;
  const plan = planAt(Infinity);
  const current = SCENARIOS[run % SCENARIOS.length];
  const status = elapsed < 14 ? "pending" : elapsed < 38 ? "in_progress" : elapsed < 57 ? "in_review" : "done";
  const previous = SCENARIOS[(run + SCENARIOS.length - 1) % SCENARIOS.length];
  const sample = (scenario: typeof current, sequence: number, state: Task["status"]) =>
    task(`task_demo_qa_${sequence}`, `FN-${208 + sequence}`, `${scenario.title}: verification matrix`, state, {
      section_ids: ["sec-qa"], assigned_tab_id: ORBIT.id, peer: "gpu-rig",
      body: `Verify ${scenario.title.toLowerCase()} across the gpu-rig matrix and laptop repro.`,
    });
  const recent = [sample(previous, run - 1, "done"), sample(current, run, status)];
  const selected = /^task_demo_qa_(-?\d+)$/.exec(openTaskId ?? "");
  if (selected && Number(selected[1]) < run - 1) {
    const number = Number(selected[1]);
    recent.unshift(sample(SCENARIOS[(number + SCENARIOS.length) % SCENARIOS.length], number, "done"));
  }
  return { ...plan, tasks: [...plan.tasks, ...recent] };
}

// ---- Board moves on the same clock ----

interface Move {
  at: number;
  add?: Task;
  task?: string;
  patch?: Partial<Task>;
}

// The board's own beat, in seconds from a plan replay: Atlas's split, the pick-ups, then the two results.
const MOVES: Move[] = [
  ...SPLIT.map((added, index) => ({ at: 1.2 + 0.3 * index, add: added })),
  { at: 3.4, task: T.resolver, patch: { status: "in_progress", assigned_tab_id: MIRA.id, lane: lane(MIRA.id, "resolver") } },
  { at: 4.1, task: T.qa, patch: { status: "in_progress", assigned_tab_id: ORBIT.id, peer: "gpu-rig", lane: lane(ORBIT.id, "qa") } },
  { at: 5.0, task: T.guide, patch: { status: "in_progress", assigned_tab_id: QUILL.id, lane: lane(QUILL.id, "docs") } },
  { at: 9.6, task: T.qa, patch: { status: "done" } },
  { at: 12.8, task: T.resolver, patch: { status: "in_review" } },
];

const BASE_PLAN: Plan = { ...FILM_PLAN, sections: SECTIONS, tasks: PRIOR };

const PLANS: Plan[] = MOVES.reduce<Plan[]>((plans, move) => {
  const prev = plans[plans.length - 1];
  const tasks = move.add ? [...prev.tasks, move.add] : prev.tasks.map((t) => (t.id === move.task ? { ...t, ...move.patch } : t));
  return [...plans, { ...prev, tasks, updated_at: FILM_NOW }];
}, [BASE_PLAN]);

/** The plan `t` seconds into its beat; the same object while nothing moves, the final state past the end. */
export function planAt(t: number): Plan {
  return PLANS[MOVES.filter((move) => move.at <= t).length];
}

export function planStep(t: number): number {
  return MOVES.filter((move) => move.at <= t).length;
}

export function planIndexOf(plan: Plan): PlanIndexEntry {
  return {
    plan_id: plan.id, file_path: `C:/Developer/fieldnote/.domios/plans/active/${plan.id}/plan.json`, canonical_project_path: "C:/Developer/fieldnote",
    owner: plan.owner, title: plan.title, status: "active", updated_at: FILM_NOW, team_id: TEAM.id,
    assigned_tab_ids: LOCAL_FIELDNOTE.map((tab) => tab.id), task_count: plan.tasks.length,
  };
}

export const PLAN_END = MOVES[MOVES.length - 1].at + 0.5;

/** The Media team's sample plan on gpu-rig, so its remote Team card has real counts to show. */
export const MEDIA_ACTIVE_PLAN: Plan = {
  ...FILM_PLAN, id: "pl_m47q2d", title: "Week 47: 2.4 launch posts", team_id: MEDIA_ID, sections: [],
  tasks: [
    task("task_mp471a", "MP-1", "Launch post draft", "done", { assigned_tab_id: MEDIA[3].id }),
    task("task_mp472b", "MP-2", "Clip from the real sync UI", "in_progress", { assigned_tab_id: MEDIA[1].id }),
    task("task_mp473c", "MP-3", "Thread on offline edits", "pending"),
  ],
};

export function mediaPlanIndexOf(plan: Plan): PlanIndexEntry {
  return { ...planIndexOf(plan), file_path: `/home/build/fieldnote/.domios/plans/active/${plan.id}/plan.json`, team_id: MEDIA_ID,
    assigned_tab_ids: MEDIA.map((tab) => tab.id) };
}

