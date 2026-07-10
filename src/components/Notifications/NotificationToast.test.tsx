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
    pane_id: null,
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
