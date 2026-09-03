// Run: npx tsx src/components/Canvas/planView/PlanListFilterBar.test.tsx
// Regression for ct#330: the plan-list search field removed the browser's
// default focus ring (outline-none) and put nothing back, and carried no
// accessible name beyond a placeholder.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PlanListFilterBar } from "./PlanListFilterBar";

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
  Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function renderBar(): Promise<{ input: HTMLInputElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <PlanListFilterBar
        statusFilter="all"
        textQuery=""
        onStatusFilter={() => {}}
        onTextQuery={() => {}}
      />,
    );
  });
  const input = host.querySelector('[data-testid="plan-list-filter-text"]') as HTMLInputElement;
  return { input, root };
}

test("filter field carries an accessible name beyond its placeholder", async () => {
  const { input } = await renderBar();
  assert(input, "filter input rendered");
  assert(input.getAttribute("aria-label") === "Filter rows", "filter input must expose aria-label");
});

test("filter field declares a visible focus-visible ring, not bare outline-none", async () => {
  const { input } = await renderBar();
  assert(
    input.className.includes("focus-visible:ring") && input.className.includes("var(--ct-accent)"),
    "outline-none with no substitute leaves keyboard focus undetectable — a ring must replace it",
  );
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`\n✓ all ${tests.length} PlanListFilterBar a11y tests green`);
}

void main();
