// Run: npx tsx src/components/Terminal/WatcherStatusBadge.test.tsx

import React, { act } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import { WatcherStatusBadge } from "./WatcherStatusBadge";

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
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
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

setupDom();

test("maps watcher statuses to tone data attributes", async () => {
  const cases: Array<[string | null, string, string]> = [
    [null, "WATCHER", "neutral"],
    ["ok", "OK", "ok"],
    ["attention", "ATTENTION", "warn"],
    ["stalled", "STALLED", "danger"],
  ];
  for (const [status, label, tone] of cases) {
    const { host, root } = await render(<WatcherStatusBadge lastStatus={status} />);
    const badge = host.querySelector('[data-testid="watcher-status-badge"]');
    assert(badge?.textContent === label, `label for ${status}`);
    assert(badge?.getAttribute("data-watcher-tone") === tone, `tone for ${status}`);
    await act(async () => root.unmount());
  }
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`PASS ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`FAIL ${name}`, error);
    }
  }
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
