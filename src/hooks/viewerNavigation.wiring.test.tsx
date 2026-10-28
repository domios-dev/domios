import assert from "node:assert/strict";
import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { BlueprintSolutionsApi } from "../components/Jobs/blueprintSolutions";

async function main() {
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "localStorage", "navigator", "HTMLElement", "HTMLButtonElement", "Element", "Node", "Event", "MouseEvent"]) Object.defineProperty(globalThis, key, { configurable: true, value: window[key as keyof Window] });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ useAppNavigation }, { useSettingsNavigation, settingsStack }, { useJobsNavigation, useMissingJobFallback }] = await Promise.all([
    import("./useAppNavigation"), import("./useSettingsNavigation"), import("./useJobsNavigation"),
  ]);
  async function mount(element: ReactElement) {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(element));
    return { render: async (next: ReactElement) => { await act(async () => root.render(next)); }, close: async () => { await act(async () => root.unmount()); } };
  }
  let app!: ReturnType<typeof useAppNavigation>;
  const workspaces = [{ id: "workspace-a" }];
  function AppProbe({ generation, tabId = null }: { generation: number; tabId?: string | null }) {
    app = useAppNavigation(generation, tabId, generation >= 1 ? workspaces : []);
    return null;
  }
  let page = await mount(<AppProbe generation={1} tabId="tab-a" />);
  await act(async () => { app.setMainView("settings"); app.setAutomationWorkspaceId("workspace-a"); });
  await page.close();
  page = await mount(<AppProbe generation={0} />);
  assert.equal(app.navigationReady, false);
  assert.equal(app.mainView, "settings");
  assert.equal(app.automationWorkspaceId, "workspace-a");
  await page.render(<AppProbe generation={0.5} tabId="cached-tab" />);
  assert.equal(app.navigationReady, false);
  assert.equal(app.automationWorkspaceId, "workspace-a", "a stale cache cannot remove a saved target");
  await page.render(<AppProbe generation={1} tabId="tab-a" />);
  assert.equal(app.navigationReady, true);
  assert.equal(app.mainView, "settings", "hydration preserves the restored panel");
  await page.render(<AppProbe generation={1} tabId="tab-b" />);
  assert.equal(app.mainView, "workspace", "a later tab choice closes the panel");
  await act(async () => { app.setAutomationWorkspaceId("removed"); });
  assert.equal(app.automationWorkspaceId, null);
  await page.close();

  let settings!: ReturnType<typeof useSettingsNavigation>;
  function SettingsProbe({ link }: { link?: "general" | "agents" }) { settings = useSettingsNavigation(link, undefined); return null; }
  let panel = await mount(<SettingsProbe />);
  await act(async () => settings[1]([{ kind: "root" }, { kind: "section", id: "agents" }, { kind: "agent-provider", providerId: "fixture" }]));
  await panel.close();
  panel = await mount(<SettingsProbe />);
  assert.deepEqual(settings[0].map((route) => route.kind), ["root", "section", "agent-provider"]);
  await panel.render(<SettingsProbe link="general" />);
  assert.deepEqual(settings[0], [{ kind: "root" }, { kind: "section", id: "general", card: undefined, helpSection: undefined }]);
  await panel.close();
  assert.deepEqual(settingsStack([{ kind: "root" }, { kind: "section", id: "removed" }]), [{ kind: "root" }]);

  const api: BlueprintSolutionsApi = {
    list: async () => [{ id: "saved", when_to_use: "fixture", how: null, uses: 0, successes: 0, last_used: 0 }],
    document: async () => ({ title: "fixture", body: "schema_version: 2\nname: Saved fixture\nsteps: []", tags: [] }),
    save: async () => ({ id: "saved", status: "saved" }), archive: async () => undefined, search: async () => [],
  };
  let jobs!: ReturnType<typeof useJobsNavigation>;
  function JobsProbe({ ready = true, exists = true }: { ready?: boolean; exists?: boolean }) {
    jobs = useJobsNavigation("fixture", null, api);
    useMissingJobFallback(jobs.selection, ready, exists, jobs.setSelection);
    return null;
  }
  let view = await mount(<JobsProbe />);
  await act(async () => jobs.setSelection({ kind: "blueprint", id: "blueprint-a" }));
  await view.close();
  view = await mount(<JobsProbe ready={false} exists={false} />);
  assert.deepEqual(jobs.selection, { kind: "blueprint", id: "blueprint-a" });
  await view.render(<JobsProbe exists={false} />);
  assert.equal(jobs.selection, null);
  const { catalogEntry } = await import("../components/Jobs/solutionCatalog");
  const saved = catalogEntry({ ...((await api.list("ws"))[0]), workspaceId: "ws" }, await api.document("saved"));
  await act(async () => jobs.setOpenedSolution(saved));
  await view.close();
  view = await mount(<JobsProbe />);
  assert.equal(jobs.openedSolution?.solution.id, "saved");
  await act(async () => jobs.setOpenedSolution(saved));
  await view.close();
  const listed = api.list;
  let land!: (solutions: Awaited<ReturnType<typeof listed>>) => void;
  api.list = () => new Promise((resolve) => { land = resolve; });
  view = await mount(<JobsProbe />);
  await act(async () => jobs.setSelection({ kind: "blueprint", id: "newer-choice" }));
  await act(async () => land(await listed("ws")));
  assert.deepEqual(jobs.selection, { kind: "blueprint", id: "newer-choice" });
  assert.equal(jobs.openedSolution, null, "late saved-solution restore cannot replace navigation");
  await act(async () => jobs.setOpenedSolution(saved));
  await view.close();
  api.list = async () => [];
  view = await mount(<JobsProbe />);
  assert.equal(jobs.openedSolution, null);
  assert.equal(jobs.selection, null, "missing saved solution falls back to Jobs");
  await view.close();
  console.log("viewerNavigation wiring: 19 assertions passed");
}

main().catch((error) => { console.error(error); process.exit(1); });
