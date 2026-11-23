import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { PlanIndexEntry } from "../../../../src/components/Canvas/plansApi";
import type { Plan, Task } from "../../../../src/components/Canvas/types";
import type { Tab, TabStatus, Workspace } from "../../../../src/lib/types";
import { CHAT_CARDS, type ChatCard } from "./chat";
import { PLAN, PLAN_INDEX } from "./plan";
import { FILM_WORKSPACE, MEMBERS } from "./team";

export const ORCHESTRATOR = MEMBERS[0];
const [, , LEDGER, MIRA, ORBIT, QUILL] = MEMBERS;

export const DRAFT = "Run the 2.4 release gate and get sync QA green on gpu-rig.";
export const TYPE_START = 1.7;
export const TYPE_END = 2.75;
export const SEND_AT = 2.95;

export type FullUiEvent = ChatCard & { at: number };

const user = (id: string, content: string): NormalizedChatMessage => ({ id, type: "user", content });
const assistant = (id: string, content: string): NormalizedChatMessage => ({ id, type: "assistant", content });

export const HISTORY: ChatCard[] = [
  CHAT_CARDS[4],
  { message: user("ui-yesterday", "Merge the schema lane once Orbit's migration run is green, then call it a day.") },
  { message: assistant("ui-merged", "Merged **lane/offline-sync-schema** into release/2.4. The v8 migration ran clean on all six test libraries.") },
  CHAT_CARDS[0],
  { message: assistant("ui-morning", "Morning. The overnight soak is green and yesterday's lanes merged clean. The resolver is next.") },
];

export const EVENTS: FullUiEvent[] = [
  { at: SEND_AT, message: user("ui-ask", DRAFT) },
  { at: 3.55, message: assistant("ui-reply", "On it. **Mira** finishes the conflict resolver, **Orbit** runs sync QA on gpu-rig. I'll post the result here.") },
  {
    at: 5.15,
    message: {
      id: "ui-report", type: "user", inboundKind: "report", fromLabel: "Mira", fromTab: MIRA.id, fromModel: "opus",
      content: "STATUS: in_review\nCHANGES: conflict resolver on lane/offline-sync-resolver at 8c41e2f\nTESTS RUN: vitest 214 resolver tests passed, 0 failed\nFOLLOWUP: Orchestrator reviews and merges",
    },
  },
  {
    at: 6.4,
    message: {
      id: "ui-mesh", type: "user", inboundKind: "mesh", fromLabel: "Orbit", fromTab: ORBIT.id, fromModel: "codex", fromMesh: "gpu-rig",
      content: "Sync QA green on gpu-rig: 48 of 48 scenarios, 0 divergent replicas. Results are cached for the gate.",
    },
  },
  { at: 7.7, ...CHAT_CARDS[1] },
];

export const PROGRESS_AT = EVENTS[4].at;

export function draftAt(t: number): string {
  if (t >= SEND_AT) return "";
  const k = Math.min(1, Math.max(0, (t - TYPE_START) / (TYPE_END - TYPE_START)));
  return DRAFT.slice(0, Math.round(DRAFT.length * k));
}

type Phase = { from: number; working: Record<string, string> };

const PHASES: Phase[] = [
  { from: 0, working: { [LEDGER.id]: "Recording yesterday's strides… (48s)" } },
  { from: SEND_AT, working: { [ORCHESTRATOR.id]: "Reading the request… (2s)", [LEDGER.id]: "Recording yesterday's strides… (51s)" } },
  {
    from: 3.9,
    working: {
      [ORCHESTRATOR.id]: "Dispatching Mira and Orbit… (5s · ↓ 2.1k tokens)",
      [LEDGER.id]: "Recording yesterday's strides… (52s)",
      [MIRA.id]: "Running the resolver tests… (4s)",
      [ORBIT.id]: "Sync QA matrix on gpu-rig (3s)",
    },
  },
  {
    from: EVENTS[2].at,
    working: {
      [ORCHESTRATOR.id]: "Reviewing Mira's report… (6s · ↓ 3.4k tokens)",
      [LEDGER.id]: "Recording yesterday's strides… (54s)",
      [ORBIT.id]: "Sync QA matrix on gpu-rig (1m 12s)",
    },
  },
  {
    from: EVENTS[3].at,
    working: {
      [ORCHESTRATOR.id]: "Merging lane/offline-sync-resolver… (8s · ↓ 4.0k tokens)",
      [LEDGER.id]: "Marking FN-203 done with its proof… (56s)",
      [QUILL.id]: "Updating the offline sync guide… (2s)",
    },
  },
];

const IDLE_LINE = "Churned for 4m 12s";
const ORCHESTRATOR_CONTEXT = { used_tokens: 84_000, window_tokens: 200_000, cost_usd: null, source: "transcript" };

export type TeamFrame = { members: Tab[]; statuses: Record<string, TabStatus>; plan: Plan; planIndex: PlanIndexEntry };

function memberFrame(phase: Phase): Pick<TeamFrame, "members" | "statuses"> {
  const members = MEMBERS.map((tab): Tab => {
    const line = phase.working[tab.id];
    const agent_state = line ? { state: "working" as const, status_line: line } : { state: "idle" as const, status_line: IDLE_LINE };
    return tab.id === ORCHESTRATOR.id ? { ...tab, agent_state, context: ORCHESTRATOR_CONTEXT } : { ...tab, agent_state };
  });
  const statuses: Record<string, TabStatus> = Object.fromEntries(members.map((tab) => [tab.id, tab.id in phase.working ? "Working" : "Idle"]));
  return { members, statuses };
}

const PROGRESS: Record<string, Task["status"]> = { "task-resolver": "done", "task-qa": "done", "task-background": "in_progress" };

function planFrame(progressed: boolean): Pick<TeamFrame, "plan" | "planIndex"> {
  if (!progressed) return { plan: PLAN, planIndex: PLAN_INDEX };
  const plan: Plan = { ...PLAN, tasks: PLAN.tasks.map((task) => (PROGRESS[task.id] ? { ...task, status: PROGRESS[task.id] } : task)) };
  return { plan, planIndex: PLAN_INDEX };
}

// Frames are memoised per phase so product providers see stable identities between seeks of the same phase.
const FRAMES = new Map<string, TeamFrame>();

export function teamAt(t: number): TeamFrame {
  const phaseIndex = PHASES.reduce((found, phase, index) => (t >= phase.from ? index : found), 0);
  const progressed = t >= PROGRESS_AT;
  const key = `${phaseIndex}:${progressed}`;
  let frame = FRAMES.get(key);
  if (!frame) FRAMES.set(key, (frame = { ...memberFrame(PHASES[phaseIndex]), ...planFrame(progressed) }));
  return frame;
}

export function orchestratorAt(t: number): { tab: Tab; status: TabStatus } {
  const { members, statuses } = teamAt(t);
  return { tab: members[0], status: statuses[members[0].id] };
}

export const WORKSPACES: Workspace[] = [
  { id: FILM_WORKSPACE.id, name: FILM_WORKSPACE.name, tabs: MEMBERS.map((tab) => tab.id), active_tab_id: ORCHESTRATOR.id, cwd: "C:/Developer/fieldnote" },
  { id: "film-binaura", name: "Binaura", tabs: [], active_tab_id: null, cwd: "C:/Developer/binaura" },
  { id: "film-rekap", name: "Rekap", tabs: [], active_tab_id: null, cwd: "C:/Developer/rekap" },
  { id: "film-site", name: "Site", tabs: [], active_tab_id: null, cwd: "C:/Developer/domios-site" },
];
