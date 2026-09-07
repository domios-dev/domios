// The field must retain a focus style and accessible name after `outline-none` removes the browser default.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import FloatingInput from "./FloatingInput";

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
  Object.defineProperty(globalThis, "HTMLTextAreaElement", { value: window.HTMLTextAreaElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function renderRevealed(): Promise<{ textarea: HTMLTextAreaElement; root: Root; host: HTMLDivElement }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <FloatingInput ptyId="pty-1" agentType="claude" tabId="tab-1" pendingPrompt={null} />,
    );
  });
  const toggle = host.querySelector("button") as HTMLButtonElement;
  assert(toggle, "collapsed toggle button rendered");
  await act(async () => toggle.click());
  const textarea = host.querySelector("textarea") as HTMLTextAreaElement;
  assert(textarea, "textarea revealed after opening");
  return { textarea, root, host };
}

test("floating message field carries an accessible name", async () => {
  const { textarea } = await renderRevealed();
  assert(textarea.getAttribute("aria-label") === "Message agent", "textarea must expose aria-label");
});

test("floating message field declares a visible focus ring, not bare outline-none", async () => {
  const { textarea } = await renderRevealed();
  assert(
    textarea.className.includes("focus-visible:ring") && textarea.className.includes("var(--ct-accent)"),
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
  console.log(`\n✓ all ${tests.length} FloatingInput a11y tests green`);
}

void main();
