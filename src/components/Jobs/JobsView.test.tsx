// Run: node scripts/run-ts-unit-tests.mjs src/components/Jobs/JobsView.test.tsx
import assert from "node:assert/strict";
import { pipelineContractGolden } from "../Automation/blueprintContract.testdata";
import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CronJob, CronRunInfo, Team } from "../../lib/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import type { Service } from "../MeshPanel/types";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { attachment, FixtureTransport, multiScenarioGraph } from "../Automation/fixtures.testdata";
import { ALL_JOBS_SCOPE, type AutomationGraph } from "../Automation/types";

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  let frameClock = 0;
  Object.assign(globalThis, {
    requestAnimationFrame: (callback: FrameRequestCallback) => { callback(frameClock += 1000); return 0; },
    cancelAnimationFrame: () => undefined,
  });

  const [{ __setInvokeForTest, __setListenForTest }, { JobsView }, { TeamJobsUnit }] = await Promise.all([
    import("../../lib/bridge"),
    import("./JobsView"),
    import("./TeamJobsUnit"),
  ]);

  function cronJob(id: string, team_id: string | null, enabled = true): CronJob {
    return {
      id, name: `name-${id}`, description: null, workspace_id: attachment.workspace_id, team_id, schedule: "*/5 * * * *", enabled,
      agent: "agent-fixture", model: null, reasoning_effort: null, task: `task-${id}`, cwd: null, role: null, report_target_tab_id: null,
      oneshot: false, max_history: 10, created_at: 1, last_run: null, next_run: null, jitter_max_ms: null,
    };
  }
  const teamCron = cronJob("cron-team", attachment.team_id);
  const unteamedCron = cronJob("cron-unteamed", null);
  const otherTeamCron = cronJob("cron-other-team", "team-b");

  function service(job: CronJob): Service {
    const allowed = { allowed: true };
    return {
      id: `cron_${job.id}`, resource_id: `cron_${job.id}`, name: job.name, kind: "cron", health_mode: "http", health_interval: 0, health: { status: "healthy" },
      push_enabled: false, schedule: job.schedule, last_run_status: "Success", registered_at: 1, updated_at: 1, enabled: true, machine_id: "machine-local", machine_name: "This machine",
      provenance: { freshness: "fresh", offline: false, fetched_at: 1 },
      capabilities: { get: allowed, history: allowed, update_schedule: allowed, delete: allowed, enable: allowed, disable: allowed, trigger: allowed, cancel: allowed },
    };
  }

  let cronList: () => Promise<CronJob[]> = async () => [teamCron, unteamedCron, otherTeamCron];
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

  const cronJobCalls: string[] = [];
  let afterCronDelete: (job: CronJob) => void = () => undefined;
  const jobActions = {
    stopCron: async (job: CronJob) => { cronJobCalls.push(`stop:${job.id}`); },
    deleteCron: async (job: CronJob) => { cronJobCalls.push(`delete:${job.id}`); afterCronDelete(job); },
  };

  const workspaces = [{ id: attachment.workspace_id, name: "Workspace A" }];
  const teams: Team[] = [{ id: attachment.team_id!, name: "Team A", lifecycle: "active", created_at: 0, workspace_id: attachment.workspace_id }];
  const workspaceGraph = (graph: AutomationGraph): AutomationGraph => ({ ...graph, all_teams: true, team_id: null });
  function workspaceTransport(graph: AutomationGraph = new FixtureTransport().graph) {
    const transport = new FixtureTransport();
    transport.graph = workspaceGraph(graph);
    transport.readOverride = async (scope) => scope.team_id === ALL_JOBS_SCOPE ? transport.graph : { ...transport.graph, all_teams: false, team_id: scope.team_id };
    return transport;
  }

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

  function jobsView(transport: FixtureTransport, initialScenarioId?: string, teamId: string | null = attachment.team_id, initialRunId?: string) {
    return <JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: attachment.workspace_id, teamId }} adapter={transport} initialScenarioId={initialScenarioId} initialRunId={initialRunId} cronActionAdapter={cronActions} jobActionAdapter={jobActions} />;
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

  test("Jobs import entry opens the placeholder form and cancel restores the list", async () => {
    const view = await mount(jobsView(workspaceTransport()));
    await act(async () => buttonWith(view.host, "Import Blueprint").click());
    assert(view.host.querySelector('form[aria-label="Import Blueprint"]'));
    assert(view.host.querySelector('input[type="file"]'));
    await act(async () => buttonWith(view.host, "Cancel import").click());
    assert.equal(view.host.querySelector('form[aria-label="Import Blueprint"]'), null);
    await view.close();
  });

  test("one Jobs list exposes the scope's Cron and stored Sentinel scenarios, never internal stages", async () => {
    const transport = workspaceTransport(multiScenarioGraph());
    const view = await mount(jobsView(transport));
    try {
      await settle(() => view.host.querySelectorAll('ul[aria-label="Jobs"] > li').length === 3);
      const [first] = transport.graph.scenarios;
      assert.ok(entry(view.host, `cron:${teamCron.id}`), "the scope's Cron is listed");
      assert.equal(entry(view.host, `cron:${unteamedCron.id}`), null, "a Cron of another Team scope stays out");
      assert.ok(entry(view.host, `scenario:${first.id}`)?.textContent?.includes("Blueprint"), "multi-stage scenario is one Blueprint entry");
      assert.ok(!view.host.textContent?.includes("Blueprint graph"), "every Blueprint is a graph; no separate graph type");
      assert.ok(!view.host.textContent?.includes("Internal stage A"), "internal stage is not a top-level job");
    } finally { await view.close(); }
  });

  test("a workspace capsule without a Team capsule merges every Team Cron and graph and derives a readable unnamed graph title", async () => {
    const transport = new FixtureTransport();
    const graph = workspaceGraph(multiScenarioGraph());
    const previousRoot = graph.scenarios[1].root;
    const otherRoot = { ...previousRoot, team_id: "team-b" };
    transport.graph = {
      ...graph,
      all_teams: true,
      team_id: null,
      scenarios: graph.scenarios.map((scenario, index) => index === 1 ? { ...scenario, name: scenario.id, root: otherRoot } : scenario),
      nodes: graph.nodes.map((node) => node.identity.id === previousRoot.id
        ? { ...node, identity: otherRoot, name: otherRoot.id, purpose: "Inspect production health", outcomes: [{ bucket: "first-ready", handlers: [] }] }
        : node),
    };
    const view = await mount(jobsView(transport, undefined, null));
    try {
      await settle(() => view.host.querySelectorAll('ul[aria-label="Jobs"] > li').length === 5);
      assert.ok(entry(view.host, `cron:${teamCron.id}`));
      assert.ok(entry(view.host, `cron:${unteamedCron.id}`));
      assert.ok(entry(view.host, `cron:${otherTeamCron.id}`));
      const unnamed = entry(view.host, `scenario:${graph.scenarios[1].id}`);
      assert.ok(unnamed?.textContent?.includes("Inspect production health"));
      assert.ok(!unnamed?.textContent?.includes(graph.scenarios[1].id), "graph ID is not row text");
      assert.ok(!unnamed?.textContent?.includes(otherRoot.id), "root node ID is not row text");
      assert.ok(unnamed?.title.includes(graph.scenarios[1].id), "graph ID stays reachable on hover");
    } finally { await view.close(); }
  });

  test("job action menus have names, arrow navigation, and return focus after Escape", async () => {
    const transport = workspaceTransport();
    const graph = transport.graph;
    const rootId = graph.scenarios[0].root.id;
    transport.graph = { ...graph, nodes: graph.nodes.map((node) => node.identity.id === rootId ? { ...node, state: "active" } : node) };
    const view = await mount(jobsView(transport));
    try {
      const title = graph.scenarios[0].name;
      await settle(() => view.host.querySelector(`[aria-label="Actions for ${title}"]`) !== null);
      const trigger = view.host.querySelector<HTMLButtonElement>(`button[aria-label="Actions for ${title}"]`)!;
      assert.equal(trigger.getAttribute("aria-haspopup"), "menu");
      await act(async () => {
        trigger.focus();
        trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
      });
      await settle(() => document.querySelector('[role="menu"]') !== null && document.activeElement?.getAttribute("role") === "menuitem");
      const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
      assert.equal(menu.getAttribute("aria-label"), `Actions for ${title}`);
      const items = menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
      assert.equal(items[0].textContent?.trim(), "Stop");
      assert.equal(items[1].textContent?.trim(), "Delete");
      await act(async () => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })));
      assert.equal(document.activeElement, items[1], "ArrowDown moves through menu items");
      await act(async () => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
      assert.equal(document.querySelector('[role="menu"]'), null);
      assert.equal(document.activeElement, trigger, "Escape restores focus to the menu button");
    } finally { await view.close(); }
  });

  test("every row has one open action and its own named actions button; Stop from that menu fires once without opening the job", async () => {
    const transport = workspaceTransport();
    const graph = transport.graph;
    const target = graph.scenarios[0];
    transport.graph = { ...graph, nodes: graph.nodes.map((node) => node.identity.id === target.root.id ? { ...node, state: "active", stop: { allowed: true, reason: null } } : node) };
    const view = await mount(jobsView(transport));
    try {
      await settle(() => entry(view.host, `scenario:${target.id}`) !== null && entry(view.host, `cron:${teamCron.id}`) !== null);
      for (const row of Array.from(view.host.querySelectorAll<HTMLElement>('ul[aria-label="Jobs"] > li'))) {
        const open = row.querySelectorAll<HTMLButtonElement>("button[data-job-key]");
        const menus = row.querySelectorAll<HTMLButtonElement>('button[aria-haspopup="menu"]');
        assert.equal(open.length, 1, "one open action per row");
        assert.equal(menus.length, 1, "one actions button per row");
        assert.equal(row.querySelectorAll("button").length, 2, "no extra open affordance");
        assert.ok(!open[0].contains(menus[0]), "the actions button is not nested in the open action");
        const name = open[0].getAttribute("aria-label")!.split(", ")[0];
        assert.ok(name, "open action is named after the job");
        assert.equal(menus[0].getAttribute("aria-label"), `Actions for ${name}`);
      }
      const trigger = view.host.querySelector<HTMLButtonElement>(`button[aria-label="Actions for ${target.name}"]`)!;
      await act(async () => trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
      await settle(() => document.querySelector('[role="menu"]') !== null);
      assert.equal(document.querySelector('[role="menu"]')!.getAttribute("aria-label"), `Actions for ${target.name}`);
      assert.ok(entry(view.host, `scenario:${target.id}`), "opening the menu leaves the list in place");
      const stop = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find((item) => item.textContent?.trim() === "Stop");
      assert.ok(stop);
      await act(async () => stop.click());
      transport.stopResult.resolve();
      await settle(() => document.querySelector('[role="menu"]') === null);
      assert.deepEqual(transport.stops.map((identity) => identity.id), [target.root.id]);
      assert.equal(view.host.querySelector("[data-edge-kind]"), null, "Stop never opens the job");
    } finally { await view.close(); }
  });

  test("Delete confirms the named graph, traps focus, restores focus on cancel, then refreshes after deletion", async () => {
    const transport = workspaceTransport();
    const graph = transport.graph;
    const target = graph.scenarios[0];
    const view = await mount(jobsView(transport));
    try {
      await settle(() => entry(view.host, `scenario:${target.id}`) !== null);
      const trigger = view.host.querySelector<HTMLButtonElement>(`button[aria-label="Actions for ${target.name}"]`)!;
      const openDelete = async () => {
        await act(async () => trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
        await settle(() => document.querySelector('[role="menu"]') !== null);
        const deleteItem = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')).find((item) => item.textContent?.trim() === "Delete");
        assert.ok(deleteItem);
        await act(async () => deleteItem.click());
      };
      await openDelete();
      const dialog = view.host.querySelector<HTMLElement>('[role="alertdialog"]');
      assert.ok(dialog);
      assert.equal(dialog.getAttribute("aria-modal"), "true");
      assert.equal(dialog.querySelector("h2")?.textContent, `Delete ${target.name}?`);
      assert.ok(dialog.textContent?.includes("and its history"));
      const cancel = dialog.querySelector<HTMLButtonElement>("button");
      const confirm = dialog.querySelector<HTMLButtonElement>("[data-dialog-confirm]");
      await settle(() => document.activeElement === cancel);
      await act(async () => cancel!.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true })));
      assert.equal(document.activeElement, confirm, "Tab stays inside the confirmation dialog");
      await act(async () => confirm!.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true })));
      assert.equal(document.activeElement, cancel, "Shift+Tab stays inside the confirmation dialog");
      await act(async () => cancel!.click());
      assert.equal(document.activeElement, trigger, "Cancel restores focus to the actions button");

      await openDelete();
      transport.graph = { ...graph, scenarios: [] };
      await act(async () => view.host.querySelector<HTMLButtonElement>("[data-dialog-confirm]")!.click());
      await settle(() => entry(view.host, `scenario:${target.id}`) === null);
      assert.deepEqual(transport.deletes.map((identity) => identity.id), [target.root.id]);
    } finally { await view.close(); }
  });

  test("Clear stopped lists the count once and removes disabled crons and stopped graphs from the current filter", async () => {
    const transport = workspaceTransport();
    const stoppedCron = cronJob("disabled-clear-me", attachment.team_id, false);
    cronList = async () => [stoppedCron];
    const graph = transport.graph;
    const rootId = graph.scenarios[0].root.id;
    transport.graph = { ...graph, nodes: graph.nodes.map((node) => node.identity.id === rootId ? { ...node, state: "stopped", stop: { allowed: false, reason: null } } : node) };
    afterCronDelete = (job) => { if (job.id === stoppedCron.id) cronList = async () => []; };
    const view = await mount(jobsView(transport));
    try {
      await settle(() => view.host.querySelector<HTMLButtonElement>('button[aria-label="Clear stopped, 2 jobs"]')?.disabled === false);
      const clear = view.host.querySelector<HTMLButtonElement>('button[aria-label="Clear stopped, 2 jobs"]')!;
      await act(async () => clear.click());
      const dialog = view.host.querySelector<HTMLElement>('[role="alertdialog"]');
      assert.ok(dialog?.textContent?.includes("2 stopped or disabled jobs"));
      transport.graph = { ...transport.graph, scenarios: [] };
      await act(async () => view.host.querySelector<HTMLButtonElement>("[data-dialog-confirm]")!.click());
      await settle(() => view.host.querySelector('ul[aria-label="Jobs"] > li') === null);
      assert.deepEqual(cronJobCalls.filter((call) => call.startsWith("delete:")), [`delete:${stoppedCron.id}`]);
      assert.deepEqual(transport.deletes.map((identity) => identity.id), [graph.scenarios[0].root.id]);
      assert.equal(clear.disabled, true, "the header action reflects the refreshed filter");
      assert.ok(document.activeElement === view.host.querySelector('section[aria-label="Jobs"]'), "empty Jobs receives focus when the refreshed bulk action is disabled");
    } finally {
      await view.close();
      cronList = async () => [teamCron, unteamedCron, otherTeamCron];
      afterCronDelete = () => undefined;
    }
  });

  test("a Cron opens its real controls and returning keeps the list entry focused", async () => {
    const view = await mount(jobsView(workspaceTransport()));
    try {
      await settle(() => entry(view.host, `cron:${teamCron.id}`) !== null);
      await act(async () => { entry(view.host, `cron:${teamCron.id}`)!.focus(); entry(view.host, `cron:${teamCron.id}`)!.click(); });
      await settle(() => cronCalls.includes("get") && cronCalls.includes("history"));
      assert.equal(buttonWith(view.host, "Trigger now").disabled, false);
      assert.equal(view.host.querySelector('[aria-label="Close cron details"]'), null, "embedded Cron never clears the global cron selection");
      await act(async () => view.host.querySelector<HTMLButtonElement>('button[aria-label="Back to All jobs"]')!.click());
      await settle(() => entry(view.host, `cron:${teamCron.id}`) !== null);
      assert.equal(document.activeElement, entry(view.host, `cron:${teamCron.id}`));
    } finally { await view.close(); }
  });

  test("a Sentinel scenario opens its graph and returning restores the opener entry", async () => {
    const transport = workspaceTransport();
    const view = await mount(jobsView(transport));
    try {
      const scenario = transport.graph.scenarios[0];
      await settle(() => entry(view.host, `scenario:${scenario.id}`) !== null);
      await act(async () => { entry(view.host, `scenario:${scenario.id}`)!.focus(); entry(view.host, `scenario:${scenario.id}`)!.click(); });
      assert.ok(view.host.querySelector("h3")?.textContent?.startsWith(scenario.name));
      assert.ok(view.host.querySelectorAll('[aria-label="Automation graph"] .bp-node').length > 0, "scenario graph rendered");
      await act(async () => view.host.querySelector<HTMLButtonElement>('button[aria-label^="Back to"]')!.click());
      assert.equal(document.activeElement, entry(view.host, `scenario:${scenario.id}`));
      assert.equal(transport.unsubscribeCount, 1, "closing the graph releases its exact-scope definition subscription");
    } finally { await view.close(); }
  });

  test("a requested scenario opens directly and its removal stays explicit with focus recovery", async () => {
    const transport = workspaceTransport(multiScenarioGraph());
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

  test("closing a Blueprint opened from chat returns there, while one opened from the list returns to the list", async () => {
    const transport = workspaceTransport();
    const scenario = transport.graph.scenarios[0];
    let exits = 0;
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} initialFilter={{ workspaceId: attachment.workspace_id, teamId: attachment.team_id }} adapter={transport} initialScenarioId={scenario.id} cronActionAdapter={cronActions} jobActionAdapter={jobActions} onClose={() => exits++} />);
    try {
      await settle(() => view.host.querySelector('button[aria-label^="Back to"]') !== null);
      await act(async () => view.host.querySelector<HTMLButtonElement>('button[aria-label^="Back to"]')!.click());
      assert.equal(exits, 1, "a directly opened Blueprint closes the Jobs surface");
    } finally { await view.close(); }
  });

  test("a run deep link opens the requested scenario with Runs expanded and that run selected", async () => {
    const transport = workspaceTransport();
    const root = transport.graph.nodes[0];
    const context = { id: "linked-run", root_node: root.identity.id, root_transition: "accepted", started_at: 10, parent: null };
    transport.graph = { ...transport.graph,
      nodes: [{ ...root, history: [{ id: "accepted", bucket: "branch-a", accepted_at: 10, causal_run: context, handlers: [] }] }, ...transport.graph.nodes.slice(1)],
      runs: [{ id: context.id, root_node: context.root_node, root_transition: context.root_transition, started_at: 10, state: "completed", incomplete: false, transitions: [{ node: root.identity, transition_id: "accepted", parent: null }] }],
    };
    const view = await mount(jobsView(transport, transport.graph.scenarios[0].id, attachment.team_id, context.id));
    try {
      await settle(() => view.host.querySelector('[aria-label="Blueprint runs"] button[aria-pressed="true"]') !== null);
      assert.ok(view.host.querySelector('[aria-controls="blueprint-run-log"][aria-expanded="true"]'));
    } finally { await view.close(); }
  });

  test("the Rust pipeline contract opens its real steps and retained run in the canvas", async () => {
    const golden = pipelineContractGolden();
    const transport = workspaceTransport(golden);
    const view = await mount(<JobsView workspaces={[{ id: golden.workspace_id, name: "Contract workspace" }]} teams={[]} adapter={transport} />);
    try {
      await settle(() => entry(view.host, "blueprint:blueprint-echo") !== null);
      await act(async () => entry(view.host, "blueprint:blueprint-echo")!.click());
      const canvas = view.host.querySelector('[role="group"][aria-label="Echo pipeline pipeline"]');
      assert.ok(canvas);
      assert.deepEqual(Array.from(canvas.querySelectorAll("article h4")).map((node) => node.textContent), ["Trigger", "command: emit", "filter: match", "Result"]);
      await act(async () => view.host.querySelector<HTMLButtonElement>('[aria-label="Blueprint view"] button[aria-expanded]')!.click());
      const run = view.host.querySelector<HTMLButtonElement>('[aria-label="Run run-echo"]');
      assert.ok(run);
      await act(async () => run.click());
      assert.equal(run.getAttribute("aria-pressed"), "true");
      assert.equal(view.host.querySelector('[aria-current="step"]'), null);
    } finally { await view.close(); }
  });

  test("a pipeline appears once in Jobs and renders its graph only after opening", async () => {
    const transport = workspaceTransport();
    transport.graph = {
      ...transport.graph,
      scenarios: [...transport.graph.scenarios, { ...transport.graph.scenarios[0], id: "blueprint-echo", name: "Previous name" }],
      blueprints: [{
        id: "blueprint-echo", name: "Echo pipeline", owner: { kind: "team", tab_id: "creator-a", team_id: attachment.team_id }, state: "running", run_id: "run-a", current_step: 0, last_event_at_ms: 100,
        trigger: { type: "event", summary: "Message received" },
        steps: [{ index: 0, name: "Echo", kind: "command", title: "Echo command", state: "running", outcome: "success", started_at_ms: 50, ended_at_ms: null, result_summary: "echoed", reason: null }],
        result: { target: { kind: "team", id: attachment.team_id }, state: "delivering", reason: null, body_ref: null },
      }],
    };
    const view = await mount(jobsView(transport));
    try {
      await settle(() => entry(view.host, "blueprint:blueprint-echo") !== null);
      assert.equal(view.host.querySelectorAll('ul[aria-label="Jobs"] [data-job-key="blueprint:blueprint-echo"]').length, 1);
      assert.equal(entry(view.host, "scenario:blueprint-echo"), null, "the matching scenario is deduplicated by identity");
      assert.equal(view.host.querySelector('[aria-label="Blueprints by owner"]'), null);
      assert.equal(view.host.querySelector('[aria-label="Echo pipeline pipeline"]'), null, "graphs stay out of the list");
      const open = entry(view.host, "blueprint:blueprint-echo");
      assert.equal(open?.getAttribute("aria-label"), "Echo pipeline, Blueprint, active");
      await act(async () => open!.click());
      assert.ok(view.host.querySelector('[aria-label="Blueprint view"]'), "opening a pipeline enters its Blueprint view");
      assert.ok(view.host.querySelector('[aria-label="Echo pipeline pipeline"]'), "the Blueprint view keeps the trigger-step-result chain");
      assert.ok(view.host.querySelector('[aria-current="step"]'));
      await act(async () => view.host.querySelector<HTMLButtonElement>('button[aria-label="Back to All jobs"]')!.click());
      assert.equal(document.activeElement, entry(view.host, "blueprint:blueprint-echo"));
    } finally { await view.close(); }
  });

  test("a pipeline deep link opens detail and returns to its list row", async () => {
    const transport = workspaceTransport();
    transport.graph = { ...transport.graph, blueprints: [{ id: "deep-pipeline", name: "Deep pipeline", owner: { kind: "team", tab_id: null, team_id: attachment.team_id }, state: "stopped", run_id: null, current_step: null, last_event_at_ms: null }] };
    const view = await mount(<JobsView workspaces={workspaces} teams={teams} adapter={transport} initialBlueprintId="deep-pipeline" />);
    try {
      await settle(() => view.host.querySelector('[aria-label="Deep pipeline pipeline"]') !== null);
      await act(async () => view.host.querySelector<HTMLButtonElement>('button[aria-label="Back to All jobs"]')!.click());
      assert.equal(document.activeElement, entry(view.host, "blueprint:deep-pipeline"));
    } finally { await view.close(); }
  });

  for (const kind of ["cron", "blueprint", "scenario"] as const) {
    test(`${kind} header Back returns to the origin once, then uses normal navigation`, async () => {
      const transport = workspaceTransport();
      transport.graph = { ...transport.graph, blueprints: [{ id: "return-pipeline", name: "Return pipeline", owner: { kind: "team", tab_id: null, team_id: attachment.team_id }, state: "stopped", run_id: null, current_step: null, last_event_at_ms: null }] };
      const activated: unknown[] = [];
      let exits = 0;
      const view = await mount(<JobsView workspaces={workspaces} teams={teams} adapter={transport}
        initialCronId={kind === "cron" ? teamCron.id : undefined}
        initialBlueprintId={kind === "blueprint" ? "return-pipeline" : undefined}
        initialScenarioId={kind === "scenario" ? transport.graph.scenarios[0].id : undefined}
        returnTo={{ workspaceId: "origin-workspace", tabId: "origin-tab" }}
        onReturnTo={(origin) => { activated.push(origin); return true; }}
        onClose={() => exits++} cronActionAdapter={cronActions} jobActionAdapter={jobActions} />);
      try {
        const detailBack = () => kind === "scenario"
          ? view.host.querySelector<HTMLButtonElement>('button[aria-label^="Back to"]')!
          : view.host.querySelector<HTMLButtonElement>('button[aria-label="Back to All jobs"]')!;
        await settle(() => kind === "scenario"
          ? view.host.querySelector('button[aria-label^="Back to"]') !== null
          : view.host.querySelector('button[aria-label="Back to All jobs"]') !== null);
        await act(async () => detailBack().click());
        assert.deepEqual(activated, [{ workspaceId: "origin-workspace", tabId: "origin-tab" }]);
        assert.equal(exits, 1);
        await act(async () => detailBack().click());
        assert.equal(activated.length, 1);
        if (kind === "scenario") assert.equal(exits, 2, "normal direct scenario Back closes Jobs");
        else assert.ok(view.host.querySelector('ul[aria-label="Jobs"]'), "normal Back reaches Jobs list");
      } finally { await view.close(); }
    });
  }

  test("an unreadable Cron list is unavailable, never an empty scope", async () => {
    const transport = workspaceTransport();
    transport.graph = { ...transport.graph, scenarios: [] };
    cronList = async () => { throw new Error("private-cron-failure"); };
    const failed = await mount(jobsView(transport));
    try {
      await settle(() => failed.host.querySelector('[role="alert"]') !== null);
      assert.equal(failed.host.querySelector('[role="status"][aria-label="No jobs"]'), null);
      assert.ok(!failed.host.textContent?.includes("private-cron-failure"));
    } finally { await failed.close(); }
    cronList = async () => [];
    const empty = await mount(jobsView(transport));
    try {
      await settle(() => empty.host.querySelector('[role="status"][aria-label="No jobs"]') !== null);
      assert.equal(empty.host.querySelector('ul[aria-label="Jobs"]'), null, "an empty scope renders no list");
    } finally {
      await empty.close();
      cronList = async () => [teamCron, unteamedCron];
    }
  });

  test("a Team Jobs disclosure lists stored Blueprints beside its Cron and Sentinel rows", async () => {
    const transport = new FixtureTransport();
    transport.graph = {
      ...multiScenarioGraph(),
      blueprints: [{ id: "team-blueprint", name: "Team pipeline", owner: { kind: "team", tab_id: "creator-a", team_id: attachment.team_id }, state: "running", run_id: "run-a", current_step: 0, last_event_at_ms: 100 }],
    };
    const opened: string[] = [];
    const blueprintEvents: unknown[] = [];
    const captureBlueprint = (event: Event) => blueprintEvents.push((event as CustomEvent).detail);
    window.addEventListener("codeterm:open-job", captureBlueprint);
    const team = { id: attachment.team_id!, name: "team-fixture", lifecycle: "active" as const, created_at: 0, workspace_id: attachment.workspace_id };
    const props = { team, plan: null, tabs: {}, managers: null, managerCount: 1, workers: null, workerCount: 0, workingWorkerCount: 0, providers: [], onOpenEffort: () => {}, crons: <span data-testid="cron-entry" />, cronCount: 1 };
    const toggleOf = (host: HTMLElement) => host.querySelector<HTMLButtonElement>('[data-testid="crons-collapse-toggle"]');
    win.localStorage.clear();
    const view = await mount(<TeamJobsUnit {...props} sentinelScope={{ workspace_id: attachment.workspace_id, team_id: attachment.team_id }} sentinelAdapter={transport} onOpenSentinel={(id) => opened.push(id)} />);
    try {
      await settle(() => toggleOf(view.host)?.textContent?.includes(String(1 + transport.graph.scenarios.length + 1)) === true);
      const toggle = toggleOf(view.host);
      await act(async () => toggle!.click());
      assert.ok(view.host.querySelector('[data-testid="cron-entry"]'));
      const rows = view.host.querySelectorAll<HTMLButtonElement>('[data-testid="sidebar-sentinel-job"]');
      assert.equal(rows.length, transport.graph.scenarios.length, "one row per stored scenario");
      await act(async () => rows[1].click());
      assert.deepEqual(opened, [transport.graph.scenarios[1].id]);
      const blueprintRow = view.host.querySelector<HTMLButtonElement>('[data-testid="sidebar-blueprint-job"]');
      assert.ok(blueprintRow, "a running Team-owned Blueprint appears in the Team disclosure");
      await act(async () => blueprintRow.click());
      assert.deepEqual(blueprintEvents, [{ workspaceId: attachment.workspace_id, target: { teamId: attachment.team_id, blueprintId: "team-blueprint" } }]);
    } finally { await view.close(); }
    window.removeEventListener("codeterm:open-job", captureBlueprint);
    const unavailable = new FixtureTransport();
    unavailable.subscribe = async () => { throw new Error("unavailable"); };
    win.localStorage.clear();
    const failed = await mount(<TeamJobsUnit {...props} cronCount={0} crons={null} sentinelScope={{ workspace_id: attachment.workspace_id, team_id: attachment.team_id }} sentinelAdapter={unavailable} onOpenSentinel={() => {}} />);
    try {
      await settle(() => toggleOf(failed.host) !== null);
      await act(async () => toggleOf(failed.host)!.click());
      assert.ok(failed.host.querySelector('[data-testid="sidebar-sentinels-unavailable"]'), "unknown Blueprint state is shown as unavailable");
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
