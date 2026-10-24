import assert from "node:assert/strict";
import { act, Profiler, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { AppConfig } from "../../lib/types";
import { pipelineContractGolden } from "../Automation/blueprintContract.testdata";
import { FixtureTransport } from "../Automation/fixtures.testdata";
import { viewerPipeline } from "../Automation/blueprintViewer.testdata";
import type { BlueprintSolutionsApi } from "../Jobs/blueprintSolutions";

async function main() {
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "localStorage", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) Object.defineProperty(globalThis, key, { configurable: true, value: window[key as keyof Window] });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  class TestMessageChannel {
    port1 = { onmessage: null as (() => void) | null, close() {} };
    port2 = { postMessage: () => queueMicrotask(() => this.port1.onmessage?.()), close() {} };
  }
  Object.defineProperty(globalThis, "MessageChannel", { configurable: true, value: TestMessageChannel });
  let frameClock = 0;
  Object.assign(globalThis, { requestAnimationFrame: (callback: FrameRequestCallback) => { callback(frameClock += 1000); return 0; }, cancelAnimationFrame: () => undefined });
  const [{ SettingsPanel }, { JobsView }, bridge, { configureOrchestrationTransport }, { orchestrationTestClient }] = await Promise.all([
    import("../Settings/SettingsPanel"), import("../Jobs/JobsView"), import("../../lib/bridge"), import("../../lib/transport/orchestrationRuntime"), import("../../lib/transport/orchestration.testdata"),
  ]);
  bridge.__setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "list_themes") return [] as T;
    if (command === "get_themes_dir") return "" as T;
    if (command === "mesh_list_peers") return { paired: [] } as T;
    throw new Error(`Unexpected command ${command}`);
  });
  bridge.__setListenForTest(async () => () => undefined);
  configureOrchestrationTransport(orchestrationTestClient(async <T,>(_method: string, path: string) => {
    if (path.startsWith("/api/crons")) return [] as T;
    throw new Error(`Unexpected path ${path}`);
  }));
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => Response.json({ consent: false });
  async function mount(element: ReactElement) {
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(element));
    return { host, close: async () => { await act(async () => root.unmount()); host.remove(); } };
  }
  async function observed(predicate: () => boolean) {
    await act(async () => {
      for (let turn = 0; turn < 100 && !predicate(); turn++) await Promise.resolve();
    });
    assert.ok(predicate(), "expected DOM state within the microtask bound");
  }
  async function deletedBlueprint() {
    const { useJobsNavigation } = await import("../../hooks/useJobsNavigation");
    const graph = { ...pipelineContractGolden(), all_teams: true, team_id: null, blueprints: [], scenarios: [], nodes: [], edges: [] };
    const solutions: BlueprintSolutionsApi = { list: async () => [], document: async () => { throw new Error("not requested"); }, archive: async () => undefined, save: async () => ({ id: "fixture", status: "saved" }), search: async () => [] };
    function Seed() { useJobsNavigation(JSON.stringify({ workspaceId: null, teamId: null }), { kind: "blueprint", id: "deleted" }, solutions); return null; }
    const seed = await mount(<Seed />);
    await seed.close();
    const storage = window.sessionStorage;
    const storageKey = Array.from({ length: storage.length }, (_, index) => storage.key(index)).find((key) => key?.startsWith("domios.navigation:"));
    assert.ok(storageKey);
    const pointerKey = `jobs:${JSON.stringify({ workspaceId: null, teamId: null })}`;
    const pointer = () => (JSON.parse(storage.getItem(storageKey)!) as Record<string, unknown>)[pointerKey];
    assert.deepEqual(pointer(), { kind: "blueprint", id: "deleted" });
    const setItem = storage.setItem.bind(storage);
    let clears = 0;
    Object.defineProperty(storage, "setItem", { configurable: true, value: (key: string, value: string) => {
      if (key === storageKey && pointer() !== null && (JSON.parse(value) as Record<string, unknown>)[pointerKey] === null) clears++;
      setItem(key, value);
    } });
    const transport = new FixtureTransport();
    let deliver!: () => void;
    const read = new Promise<typeof graph>((resolve) => { deliver = () => resolve(graph); });
    transport.readOverride = () => read;
    let commits = 0;
    const jobs = () => <Profiler id="deleted-blueprint" onRender={() => { if (++commits > 30) throw new Error("Deleted Blueprint fallback exceeded 30 commits"); }}>
      <JobsView adapter={transport} solutionsApi={solutions} machinePeers={[]} workspaces={[{ id: graph.workspace_id, name: "Workspace" }]} teams={[]} />
    </Profiler>;
    let view = await mount(jobs());
    try {
      assert.ok(view.host.querySelector('[aria-label="Blueprint view"]'), "the saved pointer waits for its graph");
      await act(async () => deliver());
      await observed(() => view.host.querySelector('[aria-label="No jobs"]') !== null);
      assert.equal(Boolean(view.host.querySelector('[aria-label="Blueprint view"]')), false);
      assert.equal(transport.reads, 1);
      assert.equal(pointer(), null);
      assert.equal(clears, 1);
      await view.close();
      view = await mount(jobs());
      await observed(() => view.host.querySelector('[aria-label="No jobs"]') !== null);
      assert.equal(Boolean(view.host.querySelector('[aria-label="Blueprint view"]')), false);
      assert.equal(clears, 1, "a reload keeps the cleared navigation pointer");
      assert.equal(transport.reads, 2);
      console.log(`navigationReload: deleted Blueprint fell back to Jobs in ${commits} commits`);
    } finally { Object.defineProperty(storage, "setItem", { configurable: true, value: setItem }); await view.close(); }
  }
  try {
    await deletedBlueprint();
    const settings = () => <SettingsPanel config={{} as AppConfig} onUpdateConfig={async () => undefined} onClose={() => undefined} />;
    let panel = await mount(settings());
    const privacy = [...panel.host.querySelectorAll("button")].find((button) => button.textContent?.includes("Privacy"));
    assert.ok(privacy);
    await act(async () => privacy.click());
    assert.equal(panel.host.querySelector('[role="dialog"]')?.getAttribute("aria-labelledby"), "codeterm-settings-title");
    await panel.close();
    panel = await mount(settings());
    assert.equal(panel.host.querySelector("#codeterm-settings-title")?.textContent, "Privacy");
    assert.ok(panel.host.querySelector('[role="switch"][aria-label="Share product analytics"]'));
    await panel.close();
    console.log("navigationReload: Settings section restored");

    const { PersistedBlueprintView } = await import("./PersistedBlueprintView");
    const blueprint = () => <PersistedBlueprintView navigationScope="fixture" pipeline={viewerPipeline()} />;
    let reader = await mount(blueprint());
    await act(async () => reader.host.querySelector<HTMLButtonElement>('[aria-label="Run past"]')!.click());
    await observed(() => reader.host.querySelector('[aria-label="Node invoke"]') !== null);
    await act(async () => reader.host.querySelector('[aria-label="Node invoke"]')!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
    await observed(() => reader.host.querySelector('[aria-label="Node return-count in invoke"]') !== null);
    await reader.close();
    reader = await mount(blueprint());
    await observed(() => reader.host.querySelector('[aria-label="Node return-count in invoke"]') !== null);
    assert.equal(reader.host.querySelector('[aria-label="Run past"]')?.getAttribute("aria-pressed"), "true");
    assert.equal(reader.host.querySelector('nav[aria-label="Blueprint path"] [aria-current="page"]')?.textContent, "typed-helper");
    await reader.close();
    console.log("navigationReload: selected run and callee restored");

    const graph = { ...pipelineContractGolden(), all_teams: true, team_id: null };
    const transport = new FixtureTransport();
    transport.graph = graph;
    transport.readOverride = async () => graph;
    const solutions: BlueprintSolutionsApi = { list: async () => [], document: async () => { throw new Error("not requested"); }, archive: async () => undefined, save: async () => ({ id: "fixture", status: "saved" }), search: async () => [] };
    const jobs = () => <JobsView adapter={transport} solutionsApi={solutions} machinePeers={[]} workspaces={[{ id: graph.workspace_id, name: "Workspace" }]} teams={[]} />;
    let view = await mount(jobs());
    await observed(() => view.host.querySelector('[data-job-key="blueprint:blueprint-echo"]') !== null);
    await act(async () => view.host.querySelector<HTMLButtonElement>('[data-job-key="blueprint:blueprint-echo"]')!.click());
    await observed(() => view.host.querySelector("h3")?.textContent === "Echo pipeline");
    await view.close();
    view = await mount(jobs());
    await observed(() => view.host.querySelector("h3")?.textContent === "Echo pipeline");
    assert.equal(Boolean(view.host.querySelector('ul[aria-label="Jobs"]')), false);
    await view.close();
    console.log("navigationReload: selected Blueprint restored");
    transport.readOverride = async () => ({ ...graph, blueprints: [], scenarios: [], nodes: [], edges: [] });
    let fallbackCommits = 0;
    view = await mount(<Profiler id="deleted-after-restoration" onRender={() => { if (++fallbackCommits > 30) throw new Error("Deleted Blueprint after restoration exceeded 30 commits"); }}>{jobs()}</Profiler>);
    await observed(() => view.host.querySelector('[aria-label="No jobs"]') !== null);
    assert.equal(Boolean(view.host.querySelector('[aria-label="Blueprint view"]')), false);
    await view.close();
    console.log("navigationReload: Settings, Blueprint, run and callee DOM restore, missing Blueprint fallback passed");
  } finally {
    globalThis.fetch = originalFetch;
    bridge.__setInvokeForTest(null); bridge.__setListenForTest(null);
  }
}

main().catch((error) => { console.error(error); process.exit(1); });
