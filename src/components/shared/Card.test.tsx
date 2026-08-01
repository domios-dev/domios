
// Run: npx tsx src/components/shared/Card.test.tsx

import React, { act } from "react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { Card } from "./Card";

const componentSource = readFileSync(fileURLToPath(import.meta.url).replace(/\.test\.tsx$/, ".tsx"), "utf8");
const globalsCss = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../styles/globals.css"), "utf8");

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

test("Card default has ct-panel-card class", async () => {
  const { host } = await render(<Card>content</Card>);
  const card = host.querySelector("div")!;
  assert(card.className.includes("ct-panel-card"), `expected ct-panel-card in ${card.className}`);
});

test("Card soft has ct-panel-card--soft class", async () => {
  const { host } = await render(<Card variant="soft">content</Card>);
  const card = host.querySelector("div")!;
  assert(card.className.includes("ct-panel-card--soft"), `expected ct-panel-card--soft in ${card.className}`);
});

test("Card interactive has role=button and tabIndex=0", async () => {
  const { host } = await render(<Card interactive onClick={() => {}}>click me</Card>);
  const card = host.querySelector("div")!;
  assert(card.getAttribute("role") === "button", "role=button on interactive card");
  assert(card.getAttribute("tabindex") === "0", "tabIndex=0 on interactive card");
});

test("Card exposes the shared press, hover, and focus contract", () => {
  assert(componentSource.includes("ct-liquid-interactive"), "interactive Card must use the shared interaction class");
  assert(/\.ct-liquid-interactive:not\(:disabled\):active\s*\{[^}]*--ct-duration-liquid-press/s.test(globalsCss), "press must use the liquid press token");
  assert(/\.ct-liquid-interactive:focus-visible,[^}]*\{[^}]*outline:/s.test(globalsCss), "focus-visible ring missing");
  assert(/\.ct-liquid-interactive:not\(:disabled\):hover\s*\{[^}]*scale\(1\.015\)/s.test(globalsCss), "hover must be a shared swell");
  assert(!/@keyframes\s+ct-liquid-interactive/.test(globalsCss), "hover must not be keyframe-driven");
  assert(componentSource.includes("useTouchOptimizedUi"), "Card must use the shared touch-mode path");
});

test("Card non-interactive has no role/tabIndex", async () => {
  const { host } = await render(<Card>static</Card>);
  const card = host.querySelector("div")!;
  assert(!card.getAttribute("role"), "no role on non-interactive card");
});

test("Card data-testid propagates", async () => {
  const { host } = await render(<Card data-testid="my-card">x</Card>);
  assert(host.querySelector('[data-testid="my-card"]'), "testid not found");
});

test("Card click fires handler", async () => {
  let clicked = false;
  const { host } = await render(<Card interactive onClick={() => { clicked = true; }}>click</Card>);
  await act(async () => { (host.querySelector("div") as HTMLElement).click(); });
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
    console.error(`\n✗ ${failed}/${tests.length} Card tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} Card tests green`);
}

void main();
