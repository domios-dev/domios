import { act, useLayoutEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { frontendSignals } from "./appState";
import { useFrontendApp, type FrontendSelection } from "./useFrontendApp";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { loadClientPageSelection, saveClientPageSelection } from "../clientPageSelection";

const rootRendered = vi.hoisted(() => vi.fn());
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
vi.mock("./browser", () => ({ frontendRootRendered: rootRendered }));

it("reports the root after selection hydration and a real React commit", async () => {
  frontendSignals.hydrate({ activeWorkspaceId: "w", activeTabId: "t", drafts: {}, scroll: {} });
  const calls: string[] = [];
  let selection: FrontendSelection = {
    generation: 0, workspaceId: "w", tabId: "t", hasWorkspace: () => true, hasTab: () => true,
    selectWorkspace: id => calls.push(id), selectTab: (_, id) => calls.push(id),
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function App() { useFrontendApp(selection); return <main aria-label="Workspace">Rendered workspace</main>; }
  rootRendered.mockImplementation(() => { expect(host.querySelector("main")?.textContent).toBe("Rendered workspace"); });
  await act(async () => root.render(<App />));
  expect(rootRendered).not.toHaveBeenCalled();
  selection = { ...selection, generation: 1 };
  await act(async () => root.render(<App />));
  expect(calls).toEqual(["w", "t"]);
  expect(rootRendered).toHaveBeenCalledOnce();
  expect(frontendSignals.safety().idle).toBe(true);
  await act(async () => root.unmount()); host.remove();
});

it("mounts the page's saved route instead of daemon focus or a stale update checkpoint", async () => {
  saveClientPageSelection({ workspaceId: "page", tabId: "reader" });
  frontendSignals.hydrate({ activeWorkspaceId: "daemon", activeTabId: "foreign", drafts: {}, scroll: {} });
  const painted: string[] = [];
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let generation = 0.5;
  const workspaces = [{ id: "page", tabs: ["reader", "other"], active_tab_id: "other" },
    { id: "daemon", tabs: ["foreign"], active_tab_id: "foreign" }];
  function Pane({ route }: { route: string }) {
    useLayoutEffect(() => { painted.push(route); }, [route]);
    return <main aria-label="Conversation" />;
  }
  function App() {
    const [workspace, setWorkspace] = useState("daemon");
    const [tab, setTab] = useState("foreign");
    const ready = useFrontendApp({ generation, workspaceId: workspace, tabId: tab,
      pageSnapshot: { active_workspace_id: "daemon", workspaces, tabs: { reader: {}, other: {}, foreign: {} } },
      hasWorkspace: () => true, hasTab: () => true, selectWorkspace: setWorkspace, selectTab: (_, id) => setTab(id) });
    return ready ? <Pane route={`${workspace}:${tab}`} />
      : <DeferredLoader state="present" role="status" aria-label="Restoring workspace" />;
  }
  rootRendered.mockImplementation(() => {});
  await act(async () => root.render(<App />));
  expect(painted).toEqual([]);
  expect(loadClientPageSelection()).toEqual({ workspaceId: "page", tabId: "reader" });
  generation = 10;
  await act(async () => root.render(<App />));
  expect(painted).toEqual(["page:reader"]);
  expect(loadClientPageSelection()).toEqual({ workspaceId: "page", tabId: "reader" });
  await act(async () => root.unmount()); host.remove();
});

it("mounts the main pane only after the saved chat route is restored", async () => {
  frontendSignals.hydrate({ activeWorkspaceId: "saved", activeTabId: "chat", drafts: {}, scroll: {} });
  const painted: string[] = [];
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function Pane({ workspace, tab }: { workspace: string; tab: string | null }) {
    useLayoutEffect(() => { painted.push(`${workspace}:${tab}`); }, [workspace, tab]);
    return <main aria-label="Conversation" />;
  }
  function App() {
    const [workspace, setWorkspace] = useState("default");
    const [tab, setTab] = useState<string | null>(null);
    const ready = useFrontendApp({ generation: 1, workspaceId: workspace, tabId: tab,
      hasWorkspace: id => id === "saved", hasTab: id => id === "chat",
      selectWorkspace: setWorkspace, selectTab: (_, id) => setTab(id) });
    return ready ? <Pane workspace={workspace} tab={tab} />
      : <DeferredLoader state="present" role="status" aria-label="Restoring workspace" />;
  }
  rootRendered.mockImplementation(() => {});
  await act(async () => root.render(<App />));
  expect(painted).toEqual(["saved:chat"]);
  expect(host.querySelector('main[aria-label="Conversation"]')).not.toBeNull();
  await act(async () => root.unmount()); host.remove();
});
