import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { Workspace } from "../../lib/types";
import { WorkspaceItem } from "./WorkspaceItem";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  HTMLInputElement: window.HTMLInputElement,
  SVGElement: window.SVGElement,
  Element: window.Element,
  Node: window.Node,
  Event: window.Event,
  MouseEvent: window.MouseEvent,
  KeyboardEvent: window.KeyboardEvent,
})) {
  Object.defineProperty(globalThis, name, { value, configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { requestAnimationFrame: (callback: FrameRequestCallback) => number }).requestAnimationFrame = (callback) => {
  callback(0);
  return 0;
};
(globalThis as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = () => {};

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const workspace: Workspace = {
  id: "workspace",
  name: "Workspace",
  tabs: [],
  active_tab_id: null,
  cwd: "/workspace",
};

async function openWorkspaceMenu(
  newSessionDisabled: boolean,
  onNewSession: () => void,
  onNewTerminal: (shell?: string) => void = () => {},
): Promise<{ root: Root; menuItem: (label: string) => HTMLButtonElement | undefined }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const noop = () => undefined;
  await act(async () => {
    root.render(
      <WorkspaceItem
        workspace={workspace}
        isActive={true}
        isExpanded={false}
        onClick={noop}
        onToggleExpand={noop}
        onClose={noop}
        onRename={noop}
        onNewTerminal={onNewTerminal}
        onNewSession={onNewSession}
        newSessionDisabled={newSessionDisabled}
        onNewAgent={noop}
        canClose={true}
        status={null}
      />,
    );
  });
  const trigger = host.querySelector('button[aria-label="Actions for Workspace"]');
  assert(trigger, "workspace actions trigger is reachable by its accessible name");
  await act(async () => {
    trigger.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true }) as unknown as Event);
  });
  const menu = document.querySelector('[role="menu"]');
  assert(menu, "workspace actions open an accessible menu");
  const menuItem = (label: string) => Array.from(menu.querySelectorAll('button[role="menuitem"]'))
    .find((button) => button.textContent?.includes(label)) as HTMLButtonElement | undefined;
  return { root, menuItem };
}

async function testGetStartedGateDisablesSessionCreation() {
  let sessions = 0;
  const { root, menuItem } = await openWorkspaceMenu(true, () => { sessions += 1; });
  const newSession = menuItem("New session");
  const quickSpawn = menuItem("Quick spawn");
  assert(newSession?.disabled, "New session is disabled while Get started is active");
  assert(quickSpawn?.disabled, "Quick spawn is disabled while Get started is active");
  await act(async () => newSession.click());
  assert(sessions === 0, "a disabled New session never opens the picker");
  await act(async () => root.unmount());
}

async function testNewSessionOpensThePickerOnce() {
  let sessions = 0;
  const { root, menuItem } = await openWorkspaceMenu(false, () => { sessions += 1; });
  const newSession = menuItem("New session");
  assert(newSession && !newSession.disabled, "New session is available after Get started");
  await act(async () => newSession.click());
  assert(sessions === 1, "New session requests the picker exactly once");
  await act(async () => root.unmount());
}

async function testDetailHeaderNavigation(countActiveJobs: boolean) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const opened: string[] = [];
  const noop = () => undefined;
  try {
    await act(async () => root.render(
      <WorkspaceItem
        workspace={workspace}
        isActive
        isExpanded
        detailHeader
        showDisclosure={false}
        countActiveJobs={countActiveJobs}
        onClick={noop}
        onToggleExpand={noop}
        onClose={noop}
        onRename={noop}
        onNewTerminal={noop}
        onNewSession={noop}
        onNewAgent={noop}
        onOpenMap={() => opened.push("map")}
        onOpenAutomation={() => opened.push("jobs")}
        canClose
        status={null}
      />,
    ));
    assert(host.querySelector('[role="heading"]')?.textContent === workspace.name, "detail header identifies the workspace");
    const map = host.querySelector<HTMLButtonElement>('button[aria-label="Map"]');
    const jobs = host.querySelector<HTMLButtonElement>('button[aria-label="Jobs"]');
    assert(map && jobs, "detail header exposes Map and Jobs by accessible name even when job reads fail");
    await act(async () => { map.click(); jobs.click(); });
    assert(opened.join(",") === "map,jobs", "detail header wires each navigation handler exactly once");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
}

async function testShellProfileMenuSelection() {
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command !== "shell_profiles") throw new Error("unavailable in test");
    return [{ id: "wsl:Ubuntu", name: "WSL: Ubuntu", executable: "wsl.exe", args: [], wsl_distro: "Ubuntu" }] as T;
  });
  const selected: (string | undefined)[] = [];
  const { root, menuItem } = await openWorkspaceMenu(false, () => {}, (shell) => selected.push(shell));
  try {
    const terminal = menuItem("New Terminal");
    assert(terminal, "terminal submenu is reachable");
    await act(async () => terminal.click());
    const profile = Array.from(document.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]')).find((button) => button.textContent?.includes("WSL: Ubuntu"));
    assert(profile, "detected shell is offered");
    await act(async () => profile.click());
    assert(selected.length === 1 && selected[0] === "wsl:Ubuntu", "profile selection fires once with the profile id");
  } finally { await act(async () => root.unmount()); }
}

async function testMissingFolderHidesSpawnAndShowsErrorMark() {
  workspace.cwd_missing = true;
  const { root, menuItem } = await openWorkspaceMenu(false, () => {});
  try {
    assert(document.querySelector('[role="img"][aria-label="Folder missing for Workspace"]'),
      "missing workspace folder has an accessible error mark");
    assert(!menuItem("New session"), "missing workspace does not offer a session");
    assert(!menuItem("Quick spawn"), "missing workspace does not offer quick spawn");
  } finally {
    await act(async () => root.unmount());
    workspace.cwd_missing = false;
  }
}

async function main() {
  __setInvokeForTest(async () => { throw new Error("bridge unavailable in test"); });
  __setListenForTest(async () => () => {});
  await testGetStartedGateDisablesSessionCreation();
  await testNewSessionOpensThePickerOnce();
  await testDetailHeaderNavigation(false);
  await testDetailHeaderNavigation(true);
  await testMissingFolderHidesSpawnAndShowsErrorMark();
  await testShellProfileMenuSelection();
  __setInvokeForTest(null);
  __setListenForTest(null);
  console.log("workspace new session menu tests: passed");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
