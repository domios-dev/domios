import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Notification } from "../../lib/types";
import { NotificationBell } from "./NotificationBell";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const item: Notification = {
  id: "notice-1",
  tab_id: "tab-tab-1",
  workspace_id: "workspace-1",
  title: "Agent needs input",
  body: "First line\nSecond line",
  level: "Warning",
  timestamp: Date.now(),
  read: false,
};

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
const calls = {
  read: [] as string[],
  deleted: [] as string[],
  navigated: [] as Array<[string, string | null]>,
  readAll: 0,
};

async function run() {
  await act(async () => {
    root.render(
      <NotificationBell
        notifications={[item]}
        onMarkRead={(id) => calls.read.push(id)}
        onMarkAllRead={() => calls.readAll++}
        onDelete={(id) => calls.deleted.push(id)}
        onNavigate={(tabId, workspaceId) => calls.navigated.push([tabId, workspaceId])}
      />,
    );
  });

  assert(host.textContent?.includes("1"), "bell should show unread count");
  await act(async () => {
    (host.querySelector('[data-testid="notification-bell"]') as HTMLElement).click();
  });
  const panel = document.querySelector('[data-testid="notification-panel"]');
  assert(panel, "bell should open a portal panel");
  assert(!panel.textContent?.includes(item.body!), "complete message should start collapsed");

  const expand = panel.querySelector<HTMLButtonElement>('[aria-label="Show complete message for Agent needs input"]');
  assert(expand, "bell entry should expose an accessible expand control");
  await act(async () => expand.click());
  assert(panel.textContent?.includes("First line\nSecond line"), "expanded entry should preserve the complete message");
  assert(expand.getAttribute("aria-expanded") === "true", "expand control should expose expanded state");

  await act(async () => expand.click());
  assert(!panel.textContent?.includes(item.body!), "expanded entry should collapse again");
  assert(expand.getAttribute("aria-expanded") === "false", "collapse should expose collapsed state");

  await act(async () => {
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Read all")) as HTMLElement).click();
    (panel.querySelector('[aria-label="Delete Agent needs input"]') as HTMLElement).click();
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Show Tab")) as HTMLElement).click();
  });
  assert(calls.readAll === 1, "Read all should use the shared notification action");
  assert(calls.deleted.join() === item.id, "Delete should remove only the selected notification");
  assert(
    calls.navigated.length === 1
      && calls.navigated[0][0] === item.tab_id
      && calls.navigated[0][1] === item.workspace_id,
    "Show Tab should navigate through the owning workspace and tab",
  );

  await act(async () => root.unmount());
  console.log("NotificationBell tests: 1/1 passed");
}

void run();
