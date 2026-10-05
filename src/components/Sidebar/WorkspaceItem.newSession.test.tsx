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
        onNewTerminal={noop}
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

async function main() {
  __setInvokeForTest(async () => { throw new Error("bridge unavailable in test"); });
  __setListenForTest(async () => () => {});
  await testGetStartedGateDisablesSessionCreation();
  await testNewSessionOpensThePickerOnce();
  __setInvokeForTest(null);
  __setListenForTest(null);
  console.log("workspace new session menu tests: passed");
}

void main().catch((error) => {
  console.error(error);
  process.exit(1);
});
