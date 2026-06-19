// Run: npx tsx packages/chat-ui/src/MarkdownBody.diff.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";

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
  window.matchMedia = () => ({
    matches: false,
    media: "",
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  });
  return window;
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

function tokenSpan(host: HTMLElement, text: string): HTMLElement | undefined {
  return Array.from(host.querySelectorAll("span")).find((node) => node.textContent === text);
}

setupDom();

const { DiffView, DIFF_HIGHLIGHT_MAX_LINES } = await import("./MarkdownBody");

test("tsx diff renders highlighted tokens on +/- backgrounds", async () => {
  const { host, root } = await render(
    <DiffView
      edits={[{ old: "const answer = 1;", new: "const answer = 42;" }]}
      language="tsx"
    />,
  );

  // The keyword `const` is tokenized into its own span by Prism.
  assert(tokenSpan(host, "const"), "highlighted token span for `const` should render");
  assert(host.textContent?.includes("answer = 42;"), "added line content should render");
  assert(host.textContent?.includes("answer = 1;"), "removed line content should render");

  // The highlighted token must live *inside* a +/- gutter row — that's what
  // "tokens on the +/- backgrounds" means structurally. (The color-mix
  // background itself is applied in real browsers but dropped by happy-dom's
  // CSS parser, so we assert the row/gutter+token coupling instead.)
  const rowWithMarker = (marker: string) =>
    Array.from(host.querySelectorAll<HTMLElement>("div")).find((d) => {
      const first = d.querySelector("span");
      return first?.textContent === marker && d.textContent?.includes("const");
    });
  const delRow = rowWithMarker("- ");
  const addRow = rowWithMarker("+ ");
  assert(delRow, "del row with `- ` gutter should render");
  assert(addRow, "add row with `+ ` gutter should render");
  assert(
    Array.from(addRow!.querySelectorAll("span")).some((n) => n.textContent === "const"),
    "add row should carry a highlighted `const` token span",
  );

  await act(async () => root.unmount());
});

test("unknown extension falls back to plain text without crashing", async () => {
  const { host, root } = await render(
    <DiffView
      edits={[{ old: "alpha beta", new: "gamma delta" }]}
      language="totally-unknown-ext"
    />,
  );

  assert(host.textContent?.includes("alpha beta"), "removed line should render in plain fallback");
  assert(host.textContent?.includes("gamma delta"), "added line should render in plain fallback");

  await act(async () => root.unmount());
});

test("large diff over the cap skips per-token highlight (plain fallback)", async () => {
  const big = Array.from({ length: DIFF_HIGHLIGHT_MAX_LINES + 5 }, (_, i) => `const v${i} = ${i};`).join("\n");
  const { host, root } = await render(
    <DiffView edits={[{ old: "", new: big }]} language="tsx" />,
  );

  // Content still renders…
  assert(host.textContent?.includes("const v0 = 0;"), "large diff content should still render");
  // …but the cap means no per-token Prism span for the `const` keyword.
  assert(!tokenSpan(host, "const"), "large diff should not produce per-token highlight spans");

  await act(async () => root.unmount());
});

const started = performance.now();
let failed = 0;
for (const [name, fn] of tests) {
  try {
    await fn();
    console.log(`✓ ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(error);
  }
}
const elapsedMs = performance.now() - started;
console.log(`DiffView highlight tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
