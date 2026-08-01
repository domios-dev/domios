
// Run: npx tsx src/components/shared/FormRow.test.tsx

import React, { act } from "react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FormRow } from "./FormRow";

const componentSource = readFileSync(fileURLToPath(import.meta.url).replace(/\.test\.tsx$/, ".tsx"), "utf8");

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

test("FormRow renders label text", async () => {
  const { host } = await render(<FormRow label="Name"><input /></FormRow>);
  const label = host.querySelector("label");
  assert(label?.textContent === "Name", `expected label 'Name', got '${label?.textContent}'`);
});

test("FormRow htmlFor wires label to input", async () => {
  const { host } = await render(
    <FormRow label="Email" htmlFor="email-input"><input id="email-input" /></FormRow>
  );
  const label = host.querySelector("label");
  assert(label?.getAttribute("for") === "email-input", "label for= not set");
});

test("FormRow shows hint text when no error", async () => {
  const { host } = await render(
    <FormRow label="Name" hint="Enter your full name"><input /></FormRow>
  );
  assert(host.textContent?.includes("Enter your full name"), "hint text not rendered");
});

test("FormRow shows error and hides hint when error present", async () => {
  const { host } = await render(
    <FormRow label="Name" hint="Enter your name" error="Required field"><input /></FormRow>
  );
  const alert = host.querySelector('[role="alert"]');
  assert(alert?.textContent === "Required field", "error text not in alert");
  assert(!host.textContent?.includes("Enter your name"), "hint must be hidden when error shows");
});

test("FormRow error has role=alert", async () => {
  const { host } = await render(
    <FormRow label="Age" error="Must be a number"><input /></FormRow>
  );
  assert(host.querySelector('[role="alert"]'), "no role=alert on error");
});

test("FormRow data-testid propagates", async () => {
  const { host } = await render(<FormRow label="X" data-testid="my-row"><input /></FormRow>);
  assert(host.querySelector('[data-testid="my-row"]'), "testid not found");
});

test("FormRow keeps form controls spatially stable", () => {
  assert(!componentSource.includes("ct-liquid-interactive"), "form fields must not scale on hover or press");
  assert(!componentSource.includes("useTouchOptimizedUi"), "static form layout needs no motion mode");
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
    console.error(`\n✗ ${failed}/${tests.length} FormRow tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} FormRow tests green`);
}

void main();
