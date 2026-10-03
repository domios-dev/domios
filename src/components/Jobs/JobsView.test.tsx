// Run: node scripts/run-ts-unit-tests.mjs src/components/Jobs/JobsView.test.tsx
import assert from "node:assert/strict";
import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CronJob, CronRunInfo } from "../../lib/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import type { Service } from "../MeshPanel/types";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { attachment, FixtureTransport, multiScenarioGraph } from "../Automation/fixtures.testdata";

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

  const [{ __setInvokeForTest, __setListenForTest }, { JobsView }, { TeamJobsUnit }] = await Promise.all([
    import("../../lib/bridge"),
    import("./JobsView"),
    import("./TeamJobsUnit"),
  ]);

  function cronJob(id: string, team_id: string | null): CronJob {
    return {
      id, name: `name-${id}`, description: null, workspace_id: attachment.workspace_id, team_id, schedule: "*/5 * * * *", enabled: true,
      agent: "agent-fixture", model: null, reasoning_effort: null, task: `task-${id}`, cwd: null, role: null, report_target_tab_id: null,
      oneshot: false, max_history: 10, created_at: 1, last_run: null, next_run: null, jitter_max_ms: null,
    };
  }
  const teamCron = cronJob("cron-team", attachment.team_id);
  const unteamedCron = cronJob("cron-unteamed", null);

  function service(job: CronJob): Service {
    const allowed = { allowed: true };
    return {
      id: `cron_${job.id}`, resource_id: `cron_${job.id}`, name: job.name, kind: "cron", health_mode: "http", health_interval: 0, health: { status: "healthy" },
      push_enabled: false, schedule: job.schedule, last_run_status: "Success", registered_at: 1, updated_at: 1, enabled: true, machine_id: "machine-local", machine_name: "This machine",
      provenance: { freshness: "fresh", offline: false, fetched_at: 1 },
      capabilities: { get: allowed, history: allowed, update_schedule: allowed, delete: allowed, enable: allowed, disable: allowed, trigger: allowed, cancel: allowed },
    };
  }

  let cronList: () => Promise<CronJob[]> = async () => [teamCron, unteamedCron];
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "cron_list") return await cronList() as T;
    if (command === "cron_get") return teamCron as T;
    if (command === "mesh_inventory_list") return { contract_version: 1, machine_id: "machine-local", machine_name: "This machine", resources: [service(teamCron)], provenance: { freshness: "fresh", offline: false, fetched_at: 1 } } satisfies MeshInventoryView as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  __setListenForTest(async () => () => undefined);

  const cronCalls: string[] = [];
  const cronActions: MeshResourceActionAdapter = {
    get: async () => { cronCalls.push("get"); return teamCron; },
    history: async () => { cronCalls.push("history"); return [] as CronRunInfo[]; },
    trigger: async () => ({ status: "Running" }),
    setEnabled: async () => ({ enabled: true }),
    cancel: async () => ({ status: "Cancelled" }),
    updateSchedule: async () => ({ schedule: teamCron.schedule }),
    remove: async () => undefined,
    regenerateToken: async () => "token",
  };

  async function settle(predicate: () => boolean) {
    await act(async () => {
      for (let tick = 0; tick < 50 && !predicate(); tick++) await Promise.resolve();
    });
    assert.ok(predicate(), "expected state observed within the microtask deadline");
  }

  async function mount(ui: ReactElement) {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(ui));
    return { host, root, close: async () => { await act(async () => root.unmount()); host.remove(); } };
  }

  function jobsView(transport: FixtureTransport, initialScenarioId?: string) {
    return <JobsView workspaceId={attachment.workspace_id} teamId={attachment.team_id} adapter={transport} initialScenarioId={initialScenarioId} cronActionAdapter={cronActions} />;
  }

  function entry(host: HTMLElement, key: string) {
    return host.querySelector<HTMLButtonElement>(`[data-job-key="${key}"]`);
  }

  function buttonWith(host: HTMLElement, text: string) {
    const found = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((candidate) => candidate.textContent?.includes(text));
    assert.ok(found, `button ${text} rendered`);
    return found;
  }

  const tests: Array<[string, () => Promise<void>]> = [];
  function test(name: string, run: () => Promise<void>) { tests.push([name, run]); }

  test("one Jobs list exposes the scope's Cron and stored Sentinel scenarios, never internal stages", async () => {
    const transport = new FixtureTransport();
    transport.graph = multiScenarioGraph();
    const view = await mount(jobsView(transport));
    try {
      await settle(() => view.host.querySelectorAll('ul[aria-label="Jobs"] > li').length === 3);
      const [first, second] = transport.graph.scenarios;
      assert.ok(entry(view.host, `cron:${teamCron.id}`), "the scope's Cron is listed");
      assert.equal(entry(view.host, `cron:${unteamedCron.id}`), null, "a Cron of another Team scope stays out");
      assert.ok(entry(view.host, `scenario:${first.id}`)?.textContent?.includes("Sentinel graph"), "multi-stage scenario is one Sentinel graph entry");
      assert.ok(!entry(view.host, `scenario:${second.id}`)?.textContent?.includes("Sentinel graph"), "single observer stays a Sentinel");
      assert.ok(!view.host.textContent?.includes("Internal stage A"), "internal stage is not a top-level job");
    } finally { await view.close(); }
  });

  test("a Cron opens its real controls and returning keeps the list entry focused", async () => {
    const view = await mount(jobsView(new FixtureTransport()));
    try {
      await settle(() => entry(view.host, `cron:${teamCron.id}`) !== null);
      await act(async () => { entry(view.host, `cron:${teamCron.id}`)!.focus(); entry(view.host, `cron:${teamCron.id}`)!.click(); });
      await settle(() => cronCalls.includes("get") && cronCalls.includes("history"));
      assert.equal(buttonWith(view.host, "Trigger now").disabled, false);
      assert.equal(view.host.querySelector('[aria-label="Close cron details"]'), null, "embedded Cron never clears the global cron selection");
      await act(async () => buttonWith(view.host, "All jobs").click());
      await settle(() => entry(view.host, `cron:${teamCron.id}`) !== null);
      assert.equal(document.activeElement, entry(view.host, `cron:${teamCron.id}`));
    } finally { await view.close(); }
  });

  test("a Sentinel scenario opens its graph and returning restores the opener entry", async () => {
    const transport = new FixtureTransport();
    const view = await mount(jobsView(transport));
    try {
      const scenario = transport.graph.scenarios[0];
      await settle(() => entry(view.host, `scenario:${scenario.id}`) !== null);
      await act(async () => { entry(view.host, `scenario:${scenario.id}`)!.focus(); entry(view.host, `scenario:${scenario.id}`)!.click(); });
      assert.ok(view.host.querySelector("h3")?.textContent?.startsWith(scenario.name));
      assert.ok(view.host.querySelectorAll("[data-edge-kind]").length > 0, "scenario graph rendered");
      await act(async () => buttonWith(view.host, "All jobs").click());
      assert.equal(document.activeElement, entry(view.host, `scenario:${scenario.id}`));
      assert.equal(transport.unsubscribeCount, 0, "list and graph share one live subscription");
    } finally { await view.close(); }
  });

  test("a requested scenario opens directly and its removal stays explicit with focus recovery", async () => {
    const transport = new FixtureTransport();
    transport.graph = multiScenarioGraph();
    const target = transport.graph.scenarios[1];
    const view = await mount(jobsView(transport, target.id));
    try {
      await settle(() => view.host.querySelector("h3")?.textContent?.startsWith(target.name) === true);
      transport.graph = { ...transport.graph, scenarios: [transport.graph.scenarios[0]] };
      await act(async () => { transport.changed!(); });
      const alert = view.host.querySelector<HTMLElement>('[role="alert"]');
      assert.ok(alert, "removed scenario is reported");
      assert.equal(document.activeElement, alert, "focus does not fall to the document body");
      assert.equal(entry(view.host, `scenario:${target.id}`), null);
      assert.ok(entry(view.host, `scenario:${transport.graph.scenarios[0].id}`), "remaining jobs stay reachable");
    } finally { await view.close(); }
  });

  test("an unreadable Cron list is unavailable, never an empty scope", async () => {
    const transport = new FixtureTransport();
    transport.graph = { ...transport.graph, scenarios: [] };
    cronList = async () => { throw new Error("private-cron-failure"); };
    const failed = await mount(jobsView(transport));
    try {
      await settle(() => failed.host.querySelector('[role="alert"]') !== null);
      assert.equal(failed.host.querySelector('[data-testid="jobs-empty"]'), null);
      assert.ok(!failed.host.textContent?.includes("private-cron-failure"));
    } finally { await failed.close(); }
    cronList = async () => [];
    const empty = await mount(jobsView(transport));
    try {
      await settle(() => empty.host.querySelector('[data-testid="jobs-empty"]') !== null);
    } finally {
      await empty.close();
      cronList = async () => [teamCron, unteamedCron];
    }
  });

  test("a Team Jobs disclosure lists stored Sentinel scenarios beside its Cron rows and opens the exact scenario", async () => {
    const transport = new FixtureTransport();
    transport.graph = multiScenarioGraph();
    const opened: string[] = [];
    const team = { id: attachment.team_id!, name: "team-fixture", lifecycle: "active" as const, created_at: 0, workspace_id: attachment.workspace_id };
    const props = { team, plan: null, tabs: {}, managers: null, managerCount: 1, workers: null, workerCount: 0, workingWorkerCount: 0, providers: [], onOpenEffort: () => {}, crons: <span data-testid="cron-entry" />, cronCount: 1 };
    const toggleOf = (host: HTMLElement) => host.querySelector<HTMLButtonElement>('[data-testid="crons-collapse-toggle"]');
    win.localStorage.clear();
    const view = await mount(<TeamJobsUnit {...props} sentinelScope={{ workspace_id: attachment.workspace_id, team_id: attachment.team_id }} sentinelAdapter={transport} onOpenSentinel={(id) => opened.push(id)} />);
    try {
      await settle(() => toggleOf(view.host)?.textContent?.includes(String(1 + transport.graph.scenarios.length)) === true);
      const toggle = toggleOf(view.host);
      await act(async () => toggle!.click());
      assert.ok(view.host.querySelector('[data-testid="cron-entry"]'));
      const rows = view.host.querySelectorAll<HTMLButtonElement>('[data-testid="sidebar-sentinel-job"]');
      assert.equal(rows.length, transport.graph.scenarios.length, "one row per stored scenario");
      await act(async () => rows[1].click());
      assert.deepEqual(opened, [transport.graph.scenarios[1].id]);
    } finally { await view.close(); }
    const unavailable = new FixtureTransport();
    unavailable.subscribe = async () => { throw new Error("unavailable"); };
    win.localStorage.clear();
    const failed = await mount(<TeamJobsUnit {...props} cronCount={0} crons={null} sentinelScope={{ workspace_id: attachment.workspace_id, team_id: attachment.team_id }} sentinelAdapter={unavailable} onOpenSentinel={() => {}} />);
    try {
      await settle(() => toggleOf(failed.host) !== null);
      await act(async () => toggleOf(failed.host)!.click());
      assert.ok(failed.host.querySelector('[data-testid="sidebar-sentinels-unavailable"]'), "unknown Sentinel state is shown as unavailable");
    } finally { await failed.close(); }
  });

  let passed = 0;
  try {
    for (const [name, run] of tests) { await run(); passed++; console.log(`✓ ${name}`); }
    console.log(`${passed}/${tests.length} Jobs view cases passed`);
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  } finally {
    __setInvokeForTest(null);
    __setListenForTest(null);
  }

}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
