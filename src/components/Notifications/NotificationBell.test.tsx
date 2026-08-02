import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Notification } from "../../lib/types";
import { __setInvokeForTest } from "../../lib/bridge";
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
  actions: [
    {
      label: "Update",
      type: "invoke",
      value: "respond_provider_update_prompt",
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

  assert(host.textContent?.includes("+1"), "bell should show the themed unread increment");
  assert(host.querySelector(".ct-notification-bell-ring"), "new unread notification should ring the bell");
  assert(host.querySelector(".ct-notification-badge-pop"), "new unread notification should pop the badge");
  await act(async () => {
    (host.querySelector('[data-testid="notification-bell"]') as HTMLElement).click();
  });
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

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  console.log("NotificationBell tests: 1/1 passed");
}

void run();
