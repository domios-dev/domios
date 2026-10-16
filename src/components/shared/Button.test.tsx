
// Run: npx tsx src/components/shared/Button.test.tsx

import React, { act } from "react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { Button } from "./Button";


type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) { tests.push([name, fn]); }
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div") as unknown as HTMLDivElement;
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(ui); });
  return { host, root };
}

test("Button uses the shared touch-mode path", () => {
  const source = readFileSync(fileURLToPath(import.meta.url).replace(/\.test\.tsx$/, ".tsx"), "utf8");
  assert(/useTouchOptimizedUi/.test(source), "Button must use the shared touch-mode path");
});

test("Button disabled is not interactive", async () => {
  const { host } = await render(<Button variant="primary" disabled>Nope</Button>);
  const btn = host.querySelector("button")!;
  assert((btn as HTMLButtonElement).disabled, "button must be disabled");
});

test("Button loading is disabled and shows BusyState", async () => {
  const { host } = await render(<Button variant="primary" loading>Working</Button>);
  const btn = host.querySelector("button")!;
  assert((btn as HTMLButtonElement).disabled, "loading button must be disabled");
  assert(btn.getAttribute("aria-busy") === "true", "aria-busy=true when loading");
  assert(btn.textContent?.includes("Working"), "loading label must remain visible");
});

test("Button data-testid propagates", async () => {
  const { host } = await render(<Button data-testid="my-btn">OK</Button>);
  assert(host.querySelector('[data-testid="my-btn"]'), "testid not found");
});

test("Button click fires handler", async () => {
  let clicked = false;
  const { host } = await render(<Button onClick={() => { clicked = true; }}>Click</Button>);
  await act(async () => {
    (host.querySelector("button") as HTMLButtonElement).click();
  });
  assert(clicked, "onClick not fired");
});

test("Button forwards its ref to the native button so callers can restore focus", async () => {
  const ref = React.createRef<HTMLButtonElement>();
  const { host } = await render(<Button ref={ref} variant="capsule" aria-label="Remove filter">Team</Button>);
  assert(ref.current === host.querySelector("button"), "ref must resolve to the rendered button");
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}: ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n✗ ${failed}/${tests.length} Button tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} Button tests green`);
}

void main();
