import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { frontendSignals } from "./appState";
import { useFrontendApp, type FrontendSelection } from "./useFrontendApp";

const rootRendered = vi.hoisted(() => vi.fn());
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
