import "../fixtures/fullui-daemon";
import "../fixtures/plugins";
import type { Tab } from "../../../../src/lib/types";
import type { TeamSpend, TeamSpendDaily } from "../../../../src/components/Canvas/missionControl/teamSpend";
import { FILM_NOW, HOUR } from "../fixtures/clock";
import { answerFilmRoute, filmRoute } from "../fixtures/daemon";
import { rememberMachineIdentities } from "../../../../src/lib/useMeshMachineIdentities";
import type { PlanEvent, TeamPlanSummary } from "../../../../src/components/Canvas/missionControl/missionControlState";
import { PLUGIN_SNAPSHOT } from "../fixtures/plugins";
import { MEMORY_EDGES, MEMORY_NODES } from "./memories";
import { DEMO_PROVIDERS } from "../../gallery/fixtures";
import { MEDIA, MEDIA_TEAM, PEERS } from "./story";

const DAYS = 7;
const day = (back: number) => new Date(FILM_NOW - back * 24 * HOUR).toISOString().slice(0, 10);
// Per-day [new input, generated, turns] in thousands of tokens, for Claude and Codex.
const CLAUDE_K = [[412, 61, 118], [388, 57, 104], [129, 18, 31], [96, 14, 22], [455, 70, 131], [503, 77, 142], [287, 44, 83]];
const CODEX_K = [[173, 22, 47], [151, 19, 41], [0, 0, 0], [38, 5, 9], [219, 29, 58], [264, 33, 66], [148, 20, 39]];

// The Media team's last plan: it has none active, so the app shows the team's own Board.
export const MEDIA_PLAN: TeamPlanSummary = {
  id: "pl_m46w7c", title: "Week 46: offline sync launch posts", team_id: MEDIA_TEAM.id, status: "archived", mode: "execution", planning_state: null,
  goal: { vector: "One launch post, a clip from the real sync UI and two threads, each with a baseline to compare against." },
  updated_at: FILM_NOW - 26 * HOUR, control_loop: null, pause_operations: [],
  task_counts: { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 6, cancelled: 1, total: 7 },
} as unknown as TeamPlanSummary;

const [LARK, MOSS, , FERN, COVE] = MEDIA;
const event = (minutesAgo: number, kind: string, author: { id: string; title: string }, text: string, task: string | null = null): PlanEvent => {
  const ts = FILM_NOW - minutesAgo * 60_000;
  return { ts, last_ts: ts, count: 1, kind, author: author.title, author_tab_id: author.id, task_id: task, task_name: task, text };
};

// What the team's last week left in its log, newest first.
export const MEDIA_EVENTS: PlanEvent[] = [
  event(26 * 60, "progress", LARK, "Week 46 closed: 6 of 7 posts shipped, the X thread moved to week 47."),
  event(27 * 60, "review", COVE, "Baseline report in: launch post 3,412 views against a 2,180 median.", "Baseline numbers"),
  event(29 * 60, "progress", FERN, "Launch post copy approved by the owner and scheduled for 09:00.", "Launch post"),
  event(31 * 60, "decision", MOSS, "Plan accepted: one cover style for every week 46 post."),
];

function rows(provider: string, model: string, table: number[][]): TeamSpendDaily[] {
  return table.map(([input, output, turns], index) => ({
    day: day(DAYS - 1 - index), team_id: MEDIA_TEAM.id, plan_id: MEDIA_PLAN.id, plan_title: MEDIA_PLAN.title, plan_status: "archived", provider, model,
    input_tokens: input * 4100, uncached_input_tokens: input * 1000, output_tokens: output * 1000, turns,
  }));
}

export const TEAM_SPEND: TeamSpend = {
  team_id: MEDIA_TEAM.id, metric: "tokens", units: "tokens", scope: "team", model: "all", from: day(DAYS - 1), to: day(0), state: "recording",
  not_recording_reason: null, plans: [{ plan_id: MEDIA_PLAN.id, title: MEDIA_PLAN.title, status: "archived" }],
  attribution: { team_plan_count: 1, unknown_plan_rows: 0, unassigned_plan_rows: 0, foreign_plan_rows_excluded: 0 },
  daily: [...rows("claude", "opus", CLAUDE_K), ...rows("codex", "gpt-6.1-sol", CODEX_K)],
  weekly_limit_points: [], weekly_limit_days: [],
};

let notice: (what: string) => void = () => undefined;
const SAMPLE_IDENTITY = { uid: "film-owner", email: null, display_name: "Sample Account", provider: "github" };

/** Routes the workspace reads; registered at mount, so they answer even after a film cut registered its own. */
export function registerWorkspaceRoutes(
  notInDemo: (what: string) => void,
  switchProvider: (tab: string, provider: string, model?: string) => void,
  meshConnected: boolean,
  tabs: Tab[],
) {
  notice = notInDemo;
  // A view's own preference writes (a popover's intro counter) stay in the page and change nothing anyone sees.
  filmRoute("PATCH", "/api/config", (_url, body) => body ?? {});
  // Keeping the machine awake would change a real setting: it says so and stays off.
  filmRoute("POST", "/api/keepawake/start", () => {
    notice("Keeping this computer awake");
    return { active: false, display: false, expires_at: null };
  });
  filmRoute("POST", "/api/keepawake/stop", () => ({ active: false, display: false, expires_at: null }));
  // Account actions belong to a real account: the sample one stays signed in and says so.
  filmRoute("POST", "/api/account/logout", () => {
    notice("Signing out");
    return { identity: SAMPLE_IDENTITY, pending: false };
  });
  filmRoute("POST", "/api/account/portal", () => {
    notice("Managing a subscription");
    return {};
  });
  const peers = meshConnected ? [...PEERS.paired, {
    ...PEERS.paired[1], instance_id: "peer-studio", instance_name: "studio",
    addresses: [{ ip: "100.64.0.24", is_tailscale: true }],
  }] : PEERS.paired;
  filmRoute("GET", "/api/mesh/peers", () => ({ ...PEERS, paired: peers }));
  // The app's mesh projection fills the machine-identity store; the film has no multiplexer, so it seeds the same store.
  rememberMachineIdentities({ ...PEERS, paired: peers, installations: [] } as unknown as Parameters<typeof rememberMachineIdentities>[0]);
  // A remote Team's reads come wrapped for its peer; the sample peers answer from the same sample routes.
  filmRoute("POST", "/api/mesh/relay-to", async (_url, body) => {
    const request = body as { method: string; path: string; body?: unknown };
    const answer = await answerFilmRoute(request.method, request.path, request.body);
    return answer.found ? { ok: true, status: 200, data: answer.value } : { ok: false, status: 404, data: null, error: "not in the film fixtures" };
  });
  const root = "C:/Developer";
  const folders = ["fieldnote", "fieldnote-site", "notes-lab", "docs-preview"].map((name) => ({ name, path: `${root}/${name}` }));
  filmRoute("GET", "/api/fs/list-dir", (url) => {
    const path = url.searchParams.get("path") || root;
    return { path, home: root, parent: path === root ? null : root, roots: [{ name: "Projects", path: root }], entries: path === root ? folders : [] };
  });
  filmRoute("POST", "/api/fs/mkdir", (_url, body) => {
    const data = body as { parent: string; name: string };
    return { path: `${data.parent}/${data.name}` };
  });
  filmRoute("GET", "/api/agents/providers", () => DEMO_PROVIDERS);
  filmRoute("GET", "/api/mem/graph/labels", (url) => {
    const ids = new Set(url.searchParams.get("ids")?.split(","));
    return MEMORY_NODES.filter((node) => ids.has(node.id)).map(({ id, title }) => ({ id, title }));
  });
  filmRoute("GET", "/api/mem/graph/edges", (url) => {
    const ids = new Set(url.searchParams.get("ids")?.split(","));
    return MEMORY_EDGES.filter((edge) => ids.has(edge.from) || ids.has(edge.to));
  });
  for (const node of MEMORY_NODES) {
    filmRoute("GET", `/api/mem/items/${node.id}`, () => ({ ...node, scope: "workspace", source: "Fieldnote", accessed_at: null }));
  }
  for (const tab of tabs) {
    filmRoute("POST", `/api/tabs/${tab.id}/agent/switch-provider`, async (_url, body) => {
      const request = body as { provider: string; model?: string };
      await new Promise<void>((resolve) => window.setTimeout(resolve, 900));
      switchProvider(tab.id, request.provider, request.model);
      return {
        tab_id: tab.id, provider: request.provider, session_id: `sample-${tab.id}`,
        previous_provider: tab.agent_type, previous_session_id: tab.agent_session_id,
        previous_history_path: "sample-history", binding_restored: true, handoff_delivered: true,
      };
    });
  }
  filmRoute("GET", "/api/mesh/status", () => ({
    enabled: true, instance_id: "peer-this", instance_name: PEERS.local_instance_name, tailscale_ip: "100.86.12.5", local_ip: "192.168.1.24", api_port: 7685,
    paired_count: peers.length,
  }));
  filmRoute("GET", "/api/mesh/inventory", () => ({ resources: [] }));
  for (const peer of peers) {
    filmRoute("GET", `/api/mesh/peers/${peer.instance_id}/services`, () => ({
      services: [], stale: false, fetched_at: FILM_NOW, peer_online: true, error: null, freshness: "fresh", partial_error: null,
    }));
  }
  filmRoute("GET", "/api/health", () => ({ instance: { display_name: PEERS.local_instance_name } }));
  filmRoute("GET", "/api/usage/history", () => []);
  filmRoute("GET", "/api/usage/projection", () => null);
  // Real names and versions, but nothing waits for an update in the demo.
  filmRoute("GET", "/api/plugins/snapshot", () => ({
    ...PLUGIN_SNAPSHOT,
    plugins: PLUGIN_SNAPSHOT.plugins.map((plugin) => ({ ...plugin, update: { available: false, reason: "current" } })),
  }));
}
