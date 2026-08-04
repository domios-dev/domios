// Run: npx tsx src/components/Notifications/NotificationToast.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { NotificationToastContainer } from "./NotificationToast";
import type { Notification } from "../../lib/types";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    value: (cb: FrameRequestCallback) => window.setTimeout(() => cb(Date.now()), 0),
    configurable: true,
  });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function notification(id: string, title: string, timestamp: number): Notification {
  return {
    id,
    title,
    timestamp,
    key: id,
    tab_id: null,
    workspace_id: null,
    body: null,
    level: "Info",
    read: false,
    sticky: true,
  };
}

test("embedded titlebar toast shows the newest notification", async () => {
  setupDom();
  const { host, root } = await render(
    <NotificationToastContainer
      toasts={[notification("old", "Old status", 1), notification("new", "New status", 2)]}
      onDismiss={() => undefined}
      embedded
    />,
  );

  assert(host.textContent?.includes("New status"), "newest toast should be visible");
  assert(!host.textContent?.includes("Old status"), "oldest toast should not occupy the embedded slot");
  await act(async () => root.unmount());
});

test("clicking an embedded toast marks it read and opens the full body", async () => {
  setupDom();
  const item = { ...notification("expand", "Detailed status", 3), body: "Line one\nLine two" };
  const marked: string[] = [];
  const { host, root } = await render(
    <NotificationToastContainer
      toasts={[item]}
      onDismiss={() => undefined}
      onMarkRead={(id) => marked.push(id)}
      embedded
    />,
  );

  await act(async () => {
    (host.querySelector('[data-testid="notification-toast"]') as HTMLElement).click();
  });
  assert(marked.join() === item.id, "click should mark the notification read");
  const expanded = document.querySelector('[data-testid="notification-toast-expanded"]');
  assert(expanded?.textContent?.includes("Line one\nLine two"), "expanded portal should preserve the full body");
  assert(host.querySelector('[data-testid="notification-toast"]'), "toast should remain visible after expansion");
  await act(async () => root.unmount());
});

test("transient toast dismisses from its animation event without timers", async () => {
  setupDom();
  const dismissed: string[] = [];
  const item = { ...notification("transient", "New message", 4), sticky: false };
  const { host, root } = await render(
    <NotificationToastContainer
      toasts={[item]}
      onDismiss={(id) => dismissed.push(id)}
      embedded
    />,
  );

  const panel = host.querySelector(".ct-toast-inline-panel");
  assert(panel?.classList.contains("ct-toast-timed"), "transient toast should use the simple timed animation");
  const animationEnd = new Event("animationend", { bubbles: true });
  Object.defineProperty(animationEnd, "animationName", { value: "ct-toast-life" });
  await act(async () => panel!.dispatchEvent(animationEnd));
  assert(dismissed.join() === item.id, "animation completion should dismiss the toast");
  await act(async () => root.unmount());
});

async function run() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (err) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(err);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`\n✓ ${tests.length}/${tests.length} NotificationToast tests passed`);
}

void run();
