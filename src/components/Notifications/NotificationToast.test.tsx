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
    severity: "Info",
    read: false,
    sticky: true,
  };
}

test("host shows at most three notifications newest first", async () => {
  setupDom();
  const { host, root } = await render(
    <NotificationToastContainer
      toasts={[notification("overflow", "Overflow", 0), notification("old", "Old status", 1), notification("middle", "Middle", 2), notification("new", "New status", 3)]}
      onDismiss={() => undefined}
    />,
  );

  assert(host.textContent?.includes("New status"), "newest toast should be visible");
  assert(Array.from(host.querySelectorAll("[data-notification-id]")).map((node) => node.getAttribute("data-notification-id")).join() === "new,middle,old", "three newest entries render in reverse arrival order");
  await act(async () => root.unmount());
});

test("clicking a toast marks it read and opens the full body", async () => {
  setupDom();
  const item = { ...notification("expand", "Detailed status", 3), body: "Line one\nLine two" };
  const marked: string[] = [];
  const { host, root } = await render(
    <NotificationToastContainer
      toasts={[item]}
      onDismiss={() => undefined}
      onMarkRead={(id) => marked.push(id)}
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

test("deadlines cap action toasts, hover pauses, and closing fires once", async () => {
  setupDom();
  const originalNow = Date.now;
  let now = 0;
  Date.now = () => now;
  const scheduled = new Map<number, { due: number; run: () => void }>();
  let timerId = 0;
  window.setTimeout = ((run: () => void, delay: number) => { const id = ++timerId; scheduled.set(id, { due: now + delay, run }); return id; }) as typeof window.setTimeout;
  window.clearTimeout = ((id: number) => scheduled.delete(id)) as typeof window.clearTimeout;
  const advance = async (elapsed: number) => {
    now += elapsed;
    await act(async () => {
      for (const [id, timer] of scheduled) if (timer.due <= now) { scheduled.delete(id); timer.run(); }
    });
  };
  const dismissed: string[] = [];
  const item = { ...notification("deadline", "Opaque status", 1), lifetime_ms: 60_000, actions: [{ type: "dismiss" as const, label: "Restore" }] };
  const { host, root } = await render(<NotificationToastContainer toasts={[item]} onDismiss={(id) => dismissed.push(id)} />);
  try {
    await advance(5000);
    await act(async () => host.querySelector('[role="status"]')!.dispatchEvent(new window.MouseEvent("mouseover", { bubbles: true }) as unknown as Event));
    await advance(30_000);
    assert(dismissed.length === 0, "hover suspends expiry");
    await act(async () => host.querySelector('[role="status"]')!.dispatchEvent(new window.MouseEvent("mouseout", { bubbles: true }) as unknown as Event));
    await advance(9999);
    assert(dismissed.length === 0, "remaining lifetime is retained");
    await advance(1);
    assert(dismissed.join() === "deadline", "configured lifetime is capped at fifteen active seconds");
    await act(async () => (host.querySelector('button[aria-label="Dismiss"]') as HTMLButtonElement).click());
    assert(dismissed.join() === "deadline", "expiry and close cannot dismiss twice");
  } finally {
    await act(async () => root.unmount());
    Date.now = originalNow;
  }
});

test("close invokes its handler once without waiting for animation", async () => {
  setupDom();
  const dismissed: string[] = [];
  const { host, root } = await render(<NotificationToastContainer toasts={[notification("close", "Opaque status", 1)]} onDismiss={(id) => dismissed.push(id)} />);
  const button = host.querySelector('button[aria-label="Dismiss"]') as HTMLButtonElement;
  await act(async () => { button.click(); button.click(); });
  assert(dismissed.join() === "close", "one close per toast");
  await act(async () => root.unmount());
});

test("all runnable actions remain available", async () => {
  setupDom();
  const item: Notification = { ...notification("actions", "Opaque status", 1), actions: [{ type: "dismiss", label: "First action" }, { type: "dismiss", label: "Second action" }] };
  const { host, root } = await render(<NotificationToastContainer toasts={[item]} onDismiss={() => undefined} />);
  assert([...host.querySelectorAll("button")].filter((button) => button.textContent?.includes("action")).length === 2, "both actions render");
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
