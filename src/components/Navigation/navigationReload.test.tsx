import assert from "node:assert/strict";
import { act, type ReactElement } from "react";
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
  try {
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
    assert.equal(reader.host.querySelector('nav[aria-label="Blueprint path"] [aria-current="page"]')?.textContent, "mem:helper");
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
    assert.equal(view.host.querySelector('ul[aria-label="Jobs"]'), null);
    await view.close();
    console.log("navigationReload: selected Blueprint restored");
    transport.readOverride = async () => ({ ...graph, blueprints: [], scenarios: [], nodes: [], edges: [] });
    view = await mount(jobs());
    await observed(() => view.host.querySelector('[aria-label="No jobs"]') !== null);
    assert.equal(view.host.querySelector("h3"), null);
    await view.close();
    console.log("navigationReload: Settings, Blueprint, run and callee DOM restore, missing Blueprint fallback passed");
  } finally {
    globalThis.fetch = originalFetch;
    bridge.__setInvokeForTest(null); bridge.__setListenForTest(null);
  }
}

main().catch((error) => { console.error(error); process.exit(1); });
