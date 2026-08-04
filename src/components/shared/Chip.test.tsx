
// Run: npx tsx src/components/shared/Chip.test.tsx

import React, { act } from "react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { Chip } from "./Chip";

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

test("Chip default has ct-small-pill class", async () => {
  const { host } = await render(<Chip>tag</Chip>);
  const chip = host.querySelector("span")!;
  assert(chip.className.includes("ct-small-pill"), `expected ct-small-pill in ${chip.className}`);
});

test("Chip accent has accent color class", async () => {
  const { host } = await render(<Chip variant="accent">accent</Chip>);
  const chip = host.querySelector("span")!;
  assert(chip.className.includes("ct-accent"), `expected ct-accent in ${chip.className}`);
});

test("Chip success has green color class", async () => {
  const { host } = await render(<Chip variant="success">done</Chip>);
  const chip = host.querySelector("span")!;
  assert(chip.className.includes("ct-green"), `expected ct-green in ${chip.className}`);
});

test("Chip danger has red color class", async () => {
  const { host } = await render(<Chip variant="danger">error</Chip>);
  const chip = host.querySelector("span")!;
  assert(chip.className.includes("ct-red"), `expected ct-red in ${chip.className}`);
});

test("Chip data-testid propagates", async () => {
  const { host } = await render(<Chip data-testid="my-chip">x</Chip>);
  assert(host.querySelector('[data-testid="my-chip"]'), "testid not found");
});

test("Chip renders children text", async () => {
  const { host } = await render(<Chip>pending</Chip>);
  const chip = host.querySelector("span")!;
  assert(chip.textContent === "pending", `expected 'pending', got '${chip.textContent}'`);
});

test("Chip exposes the shared press, hover, and focus contract", () => {
  assert(componentSource.includes("ct-liquid-interactive"), "Chip must use the shared interaction class");
  assert(/\.ct-liquid-interactive:not\(:disabled\):active\s*\{[^}]*--ct-duration-liquid-press/s.test(globalsCss), "press must use the liquid press token");
  assert(/\.ct-liquid-interactive:focus-visible,[^}]*\{[^}]*outline:/s.test(globalsCss), "focus-visible ring missing");
  assert(/\.ct-liquid-interactive\s*\{[^}]*--ct-ease-hover-swell/s.test(globalsCss), "hover must use the swell easing token");
  assert(!/@keyframes\s+ct-liquid-interactive/.test(globalsCss), "hover must not be keyframe-driven");
  assert(componentSource.includes("useTouchOptimizedUi"), "Chip must use the shared touch-mode path");
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
    console.error(`\n✗ ${failed}/${tests.length} Chip tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} Chip tests green`);
}

void main();
