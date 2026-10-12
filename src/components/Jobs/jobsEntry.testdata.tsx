import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import assert from "node:assert/strict";
import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CronJob, Team } from "../../lib/types";
import { attachment, fixtureGraph, fixtureNode, FixtureTransport, identity } from "../Automation/fixtures.testdata";
import type { AutomationGraph, BlueprintSnapshot } from "../Automation/types";

export function installDom() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, {
    IS_REACT_ACT_ENVIRONMENT: true,
    requestAnimationFrame: (callback: FrameRequestCallback) => { callback(0); return 0; },
    cancelAnimationFrame: () => undefined,
  });
  return win;
}

export const workspaceA = attachment.workspace_id;
export const workspaceB = "workspace-b";
export const workspaces = [{ id: workspaceA, name: "Workspace A" }, { id: workspaceB, name: "Workspace B" }];
export const teamA: Team = { id: attachment.team_id!, name: "Team A", lifecycle: "active", created_at: 0, workspace_id: workspaceA };
export const remoteTeam: Team = { id: "remote-team", name: "Remote Team", lifecycle: "active", created_at: 0, workspace_id: workspaceA, remote_peer_name: "peer" };
export const teams = [teamA, remoteTeam];

function cronJob(id: string, workspace_id: string, team_id: string | null): CronJob {
  return {
    id, name: `name-${id}`, description: null, workspace_id, team_id, schedule: "*/5 * * * *", enabled: true,
    agent: "agent-fixture", model: null, reasoning_effort: null, task: `task-${id}`, cwd: null, role: null, report_target_tab_id: null,
    oneshot: false, max_history: 10, created_at: 1, last_run: null, next_run: null, jitter_max_ms: null,
  };
}
export const teamCron = cronJob("cron-team-a", workspaceA, teamA.id);
export const unteamedCron = cronJob("cron-unteamed-a", workspaceA, null);
export const foreignCron = cronJob("cron-workspace-b", workspaceB, null);

export const graphA: AutomationGraph = { ...fixtureGraph(), all_teams: true, team_id: null };
const rootB = { ...identity("blueprint", "sentinel-workspace-b"), workspace_id: workspaceB, team_id: null };
export const graphB: AutomationGraph = {
  workspace_id: workspaceB, team_id: null, all_teams: true, revision: "revision-b",
  scenarios: [{ id: "scenario-workspace-b", name: "Workspace B watcher", root: rootB }],
  nodes: [{ ...fixtureNode(rootB, "Watcher B"), blueprint_definition: { source: "stored_agent_definition", definition_id: rootB.id, creator_tab_id: rootB.owner_tab_id! } }],
  edges: [],
};
export function pipelineFixture(id: string, ownerTeamId: string | null): BlueprintSnapshot {
  return { id, name: `Pipeline ${id}`, owner: { kind: ownerTeamId ? "team" : "tab", tab_id: "owner-a", team_id: ownerTeamId }, state: "stopped", run_id: null, current_step: null, last_event_at_ms: null };
}
export const scenarioA = `scenario:${graphA.scenarios[0].id}`;
export const scenarioB = `scenario:${graphB.scenarios[0].id}`;

/** Machine state behind the bridge: `cron_list` and the host workspace graph read from it. */
export const machine = {
  crons: [teamCron, unteamedCron, foreignCron] as CronJob[],
  graphs: { [workspaceA]: graphA, [workspaceB]: graphB } as Record<string, AutomationGraph>,
  cronReads: [] as unknown[],
  reset() {
    this.crons = [teamCron, unteamedCron, foreignCron];
    this.graphs = { [workspaceA]: graphA, [workspaceB]: graphB };
  },
};

function graphOf(workspaceId: unknown): AutomationGraph {
  const graph = machine.graphs[String(workspaceId)];
  if (!graph) throw new Error("unknown workspace");
  return graph;
}

export async function installBridge() {
  const bridge = await import("../../lib/bridge");
  configureOrchestrationTransport(orchestrationTestClient(async <T,>(_method: string, path: string) => {
    const url = new URL(path, "http://fixture.test");
    const workspaceId = url.searchParams.get("workspace_id");
    if (url.pathname === "/api/crons") {
      machine.cronReads.push(workspaceId === null ? {} : { workspaceId });
      return machine.crons.filter((job) => workspaceId === null || job.workspace_id === workspaceId) as T;
    }
    if (url.pathname === "/api/automation/graph/workspace") return graphOf(workspaceId) as T;
    throw new Error(`Unexpected HTTP path: ${path}`);
  }));
  bridge.__setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    if (command === "cron_list") {
      machine.cronReads.push(args);
      const workspaceId = args?.workspaceId;
      return machine.crons.filter((job) => workspaceId === undefined || job.workspace_id === workspaceId) as T;
    }
    if (command === "automation_graph_workspace_snapshot") return graphOf(args?.workspaceId) as T;
    if (command === "list_themes") return [] as T;
    if (command === "get_themes_dir") return "" as T;
    if (command === "get_autostart_enabled") return false as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  bridge.__setListenForTest(async () => () => undefined);
  return () => { bridge.__setInvokeForTest(null); bridge.__setListenForTest(null); };
}

export function machineTransport() {
  const transport = new FixtureTransport();
  transport.readOverride = async (scope) => graphOf(scope.workspace_id);
  return transport;
}

export async function settle(predicate: () => boolean) {
  await act(async () => {
    for (let tick = 0; tick < 80 && !predicate(); tick++) await Promise.resolve();
  });
  assert.ok(predicate(), "expected state observed within the microtask deadline");
}

export async function mount(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root, close: async () => { await act(async () => root.unmount()); host.remove(); } };
}

export const named = (host: ParentNode, name: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`);
export const entry = (host: HTMLElement, key: string) => host.querySelector<HTMLButtonElement>(`[data-job-key="${key}"]`);
export const renderedJobs = (host: HTMLElement) => Array.from(host.querySelectorAll<HTMLButtonElement>('ul[aria-label="Jobs"] [data-job-key]')).map((button) => button.dataset.jobKey).sort();
/** The view's header controls by accessible name: the structure both entry points must share. */
export const viewControls = (host: HTMLElement) => {
  const section = host.querySelector<HTMLElement>('section[aria-label="Jobs"]');
  assert.ok(section, "Jobs view rendered");
  return {
    controls: Array.from(section.querySelectorAll<HTMLButtonElement>("header button")).map((button) => button.getAttribute("aria-label")),
    jobs: renderedJobs(section),
  };
};
export const removeCapsules = (host: HTMLElement) => Array.from(host.querySelectorAll<HTMLButtonElement>('[role="group"][aria-label="Job filters"] button[aria-label^="Remove filter: "]'));

export async function runCases(label: string, tests: Array<[string, () => Promise<void>]>, cleanup: () => void) {
  let passed = 0;
  try {
    for (const [name, run] of tests) { await run(); passed++; console.log(`✓ ${name}`); }
    console.log(`${passed}/${tests.length} ${label} cases passed`);
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  } finally {
    cleanup();
  }
}
