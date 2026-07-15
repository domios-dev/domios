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
  pane_id: "tab-pane-1",
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
const calls = { read: [] as string[], deleted: [] as string[], navigated: [] as string[], readAll: 0 };

async function run() {
  await act(async () => {
    root.render(
      <NotificationBell
        notifications={[item]}
        onMarkRead={(id) => calls.read.push(id)}
        onMarkAllRead={() => calls.readAll++}
        onDelete={(id) => calls.deleted.push(id)}
        onNavigate={(id) => calls.navigated.push(id)}
      />,
    );
  });

  assert(host.textContent?.includes("1"), "bell should show unread count");
  await act(async () => {
    (host.querySelector('[data-testid="notification-bell"]') as HTMLElement).click();
  });
  const panel = document.querySelector('[data-testid="notification-panel"]');
  assert(panel, "bell should open a portal panel");
  assert(panel.textContent?.includes("First line\nSecond line"), "panel should preserve the full message");

  await act(async () => {
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Read all")) as HTMLElement).click();
    (panel.querySelector('[aria-label="Delete Agent needs input"]') as HTMLElement).click();
    (Array.from(panel.querySelectorAll("button")).find((button) => button.textContent?.includes("Show Tab")) as HTMLElement).click();
  });
  assert(calls.readAll === 1, "Read all should use the shared notification action");
  assert(calls.deleted.join() === item.id, "Delete should remove only the selected notification");
  assert(calls.navigated.join() === item.pane_id, "Show Tab should navigate to the linked tab");

  await act(async () => root.unmount());
  console.log("NotificationBell tests: 1/1 passed");
}

void run();
