import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Notification } from "../../lib/types";
import { __setInvokeForTest } from "../../lib/bridge";
import { NotificationBell } from "./NotificationBell";

let checked = 0;

function assert(condition: unknown, message: string): asserts condition {
  checked++;
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
  severity: "Warning",
  timestamp: Date.now(),
  read: false,
  actions: [
    {
      label: "Update",
      type: "invoke",
      command: "respond_provider_update_prompt",
      args: { promptId: "prompt-1", accept: true },
    },
  ],
};

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
const calls = {
  read: [] as string[],
  deleted: [] as string[],
  navigated: [] as Array<[string, string | null]>,
  readAll: 0,
  clearedAll: 0,
  invoked: [] as Array<{ command: string; args: unknown }>,
};

async function run() {
  __setInvokeForTest(async (command, args) => {
    calls.invoked.push({ command, args });
    return undefined as never;
  });
  await act(async () => {
    root.render(
      <NotificationBell
        notifications={[item]}
        onMarkRead={(id) => calls.read.push(id)}
        onMarkAllRead={() => calls.readAll++}
        onDelete={(id) => calls.deleted.push(id)}
        onClearAll={() => calls.clearedAll++}
        onNavigate={(tabId, workspaceId) => calls.navigated.push([tabId, workspaceId])}
      />,
    );
  });

  const bell = host.querySelector('[data-testid="notification-bell"]') as HTMLElement;
  assert(host.querySelector('[data-testid="notification-unread-dot"]'), "arrival should render the dot");
  assert(!/\d/.test(host.textContent ?? ""), "the bell should render no numeric count at all");
  assert(host.querySelector(".ct-notification-bell-ring"), "arrival should set the bell animation state");
  assert(host.querySelector(".ct-notification-dot-pop"), "arrival should set the dot animation state");
  const status = host.querySelector('[role="status"]');
  assert(status, "unread state should stay semantically available without a count");
  assert(status.textContent === "Unread notifications", "the status region should name the unread state");
  assert(
    bell.getAttribute("aria-label") === "Notifications"
      && bell.getAttribute("aria-describedby") === status.getAttribute("id"),
    "the bell should keep its accessible name and describe the unread state",
  );

  await act(async () => {
    bell.click();
  });
  assert(!host.querySelector('[data-testid="notification-unread-dot"]'), "looking should remove the dot");
  assert(!host.querySelector(".ct-notification-bell-ring"), "looking should clear the animation state");
  assert(!bell.getAttribute("aria-describedby"), "a looked-at bell should describe nothing unread");
  assert(host.querySelector('[role="status"]')?.textContent === "", "the status region should fall silent");
  const panel = document.querySelector('[data-testid="notification-panel"]');
  assert(panel, "bell should open a portal panel");
  assert(panel.className.includes("max-w-[calc(100vw-16px)]"), "panel should stay within the mobile viewport width");
  assert(panel.getAttribute("style")?.includes("100dvh"), "panel height should follow the dynamic mobile viewport");
  assert(panel.querySelector('[aria-label="Close notifications"]'), "panel should expose a visible close control");
  const update = Array.from(panel.querySelectorAll("button")).find(
    (button) => button.textContent === "Update",
  );
  assert(update, "actionable notification should expose Update inside the bell panel");
  await act(async () => update.click());
  assert(
    calls.invoked[0]?.command === "respond_provider_update_prompt",
    "Update should invoke the provider update command",
  );
  assert(panel.textContent?.includes(item.body!), "the newest complete message should start open");

  const expand = panel.querySelector<HTMLButtonElement>('[aria-label="Show complete message for Agent needs input"]');
  assert(expand, "bell entry should expose an accessible expand control");
  await act(async () => expand.click());
  assert(!panel.textContent?.includes(item.body!), "closing should hide the complete message");
  assert(expand.getAttribute("aria-expanded") === "false", "collapse should expose collapsed state");

  await act(async () => expand.click());
  assert(panel.textContent?.includes("First line\nSecond line"), "reopening should preserve the complete message");
  assert(expand.getAttribute("aria-expanded") === "true", "reopening should expose expanded state");

  await act(async () => {
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Read all")) as HTMLElement).click();
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Clear all")) as HTMLElement).click();
    (panel.querySelector('[aria-label="Delete Agent needs input"]') as HTMLElement).click();
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Show Tab")) as HTMLElement).click();
  });
  assert(calls.readAll === 1, "Read all should use the shared notification action");
  assert(calls.clearedAll === 1, "Clear all should remove notification history through the shared action");
  assert(calls.deleted.join() === item.id, "Delete should remove only the selected notification");
  assert(
    calls.navigated.length === 1
      && calls.navigated[0][0] === item.tab_id
      && calls.navigated[0][1] === item.workspace_id,
    "Show Tab should navigate through the owning workspace and tab",
  );

  const later: Notification = { ...item, id: "notice-2", timestamp: item.timestamp + 1 };
  await act(async () => {
    root.render(
      <NotificationBell
        notifications={[item, later]}
        onMarkRead={(id) => calls.read.push(id)}
        onMarkAllRead={() => calls.readAll++}
        onDelete={(id) => calls.deleted.push(id)}
        onClearAll={() => calls.clearedAll++}
        onNavigate={(tabId, workspaceId) => calls.navigated.push([tabId, workspaceId])}
      />,
    );
  });
  assert(host.querySelector('[data-testid="notification-unread-dot"]'), "a later arrival should raise the dot again");
  assert(host.querySelector(".ct-notification-bell-ring"), "a later arrival should re-ring the bell");
  assert(!/\d/.test(host.textContent ?? ""), "two unread notifications still render no count");

  await act(async () => {
    root.render(
      <NotificationBell
        notifications={[{ ...item, read: true }, { ...later, read: true }]}
        onMarkRead={(id) => calls.read.push(id)}
        onMarkAllRead={() => calls.readAll++}
        onDelete={(id) => calls.deleted.push(id)}
        onClearAll={() => calls.clearedAll++}
        onNavigate={(tabId, workspaceId) => calls.navigated.push([tabId, workspaceId])}
      />,
    );
  });
  assert(!host.querySelector('[data-testid="notification-unread-dot"]'), "no unread means no dot");
  assert(!host.querySelector(".ct-notification-bell-ring"), "no unread means no animation state");
  assert(
    (host.querySelector('[data-testid="notification-bell"]') as HTMLElement).getAttribute("aria-label")
      === "Notifications",
    "the accessible name survives the count removal",
  );

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  console.log(`NotificationBell tests: ${checked} assertions passed`);
}

void run();
