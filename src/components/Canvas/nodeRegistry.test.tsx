// Run: npx tsx src/components/Canvas/nodeRegistry.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  fallbackRenderer,
  getNodeRenderer,
  registerNodeRenderer,
  type NodeRenderer,
} from "./nodeRegistry";
import type { LaidOutNode } from "./layout";

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
  const window = new Window({ url: "http://localhost/node-registry" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

const node = (kind: string): LaidOutNode => ({
  id: `n-${kind}`,
  kind,
  planId: "p",
  box: { x: 0, y: 0, width: 200, height: 64 },
  iteration: null,
  manual: false,
});

test("built-in kinds resolve to a registered renderer (not the fallback)", () => {
  for (const kind of ["spec", "plan", "task"]) {
    assert(getNodeRenderer(kind) !== fallbackRenderer, `${kind} should be registered`);
  }
});

test("unknown kind resolves to the fallback renderer", () => {
  assert(getNodeRenderer("totally-unknown-kind") === fallbackRenderer, "unknown → fallback");
});

test("provenance kinds note and decision resolve to registered renderers (not fallback)", () => {
  for (const kind of ["note", "decision"]) {
    assert(getNodeRenderer(kind) !== fallbackRenderer, `${kind} should be registered`);
  }
});

test("registerNodeRenderer adds a new kind without touching core", () => {
  const custom: NodeRenderer = () => null;
  registerNodeRenderer("prototype", custom);
  assert(getNodeRenderer("prototype") === custom, "custom renderer retrievable");
});

test("note renderer mounts NoteCard; unknown kind falls back without crashing", async () => {
  const noteOut = getNodeRenderer("note")({
    node: node("note"),
    model: {
      id: "n-note",
      kind: "note",
      canvas: { x: 0, y: 0, collapsed: false },
      content: "**hello note**",
      links: [],
    },
    style: { position: "absolute" },
    lod: "near",
      world: "W3",
  });
  const decisionOut = getNodeRenderer("decision")({
    node: node("decision"),
    model: {
      id: "n-decision",
      kind: "decision",
      canvas: { x: 0, y: 0, collapsed: false },
      content: "Ship v1",
      links: [],
    },
    style: { position: "absolute" },
    lod: "near",
      world: "W3",
  });
  const fbOut = getNodeRenderer("mystery")({
    node: node("mystery"),
    model: { id: "n-mystery", kind: "mystery", canvas: { x: 0, y: 0, collapsed: false }, content: "**md**", links: [] },
    style: { position: "absolute" },
    lod: "near",
      world: "W3",
  });

  const { host, root } = await render(
    <>
      {noteOut}
      {decisionOut}
      {fbOut}
    </>,
  );
  assert(host.querySelector('[data-testid="canvas-note-card"]'), "NoteCard mounted");
  assert(host.querySelector('[data-testid="canvas-decision-card"]'), "DecisionCard mounted");
  assert(host.querySelector('[data-testid="canvas-fallback-node"]'), "fallback node mounted");
  assert(
    host.querySelector('[data-testid="canvas-fallback-node"]')?.getAttribute("data-node-kind") === "mystery",
    "fallback carries the unknown kind",
  );
  await act(async () => root.unmount());
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
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
