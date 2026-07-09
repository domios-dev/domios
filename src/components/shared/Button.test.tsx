
// Run: npx tsx src/components/shared/Button.test.tsx

import React, { act } from "react";
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

test("Button primary has ct-btn-primary class", async () => {
  const { host } = await render(<Button variant="primary">Save</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-primary"), `expected ct-btn-primary in ${btn.className}`);
});

test("Button ghost has ct-btn-ghost class", async () => {
  const { host } = await render(<Button variant="ghost">Cancel</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-ghost"), `expected ct-btn-ghost in ${btn.className}`);
});

test("Button accent has ct-btn-accent class", async () => {
  const { host } = await render(<Button variant="accent">Action</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-accent"), `expected ct-btn-accent in ${btn.className}`);
});

test("Button danger has ct-btn-danger class", async () => {
  const { host } = await render(<Button variant="danger">Delete</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-danger"), `expected ct-btn-danger in ${btn.className}`);
});

test("Button size lg has ct-btn-lg class", async () => {
  const { host } = await render(<Button variant="primary" size="lg">Big</Button>);
  const btn = host.querySelector("button")!;
  assert(btn.className.includes("ct-btn-lg"), `expected ct-btn-lg in ${btn.className}`);
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
  const dots = host.querySelectorAll(".ct-phase-dots");
  assert(dots.length > 0, "BusyState dots must render in loading state");
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
