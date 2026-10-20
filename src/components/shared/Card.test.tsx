
// Run: npx tsx src/components/shared/Card.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { Card } from "./Card";


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

test("Card interactive has role=button and tabIndex=0", async () => {
  const { host } = await render(<Card interactive onClick={() => {}}>click me</Card>);
  const card = host.querySelector("div")!;
  assert(card.getAttribute("role") === "button", "role=button on interactive card");
  assert(card.getAttribute("tabindex") === "0", "tabIndex=0 on interactive card");
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

test("wheel over a card, title or subtitle reaches its containing list uncancelled", async () => {
  let received = 0;
  const { host, root } = await render(
    <div role="list" aria-label="Cards" style={{ overflowY: "auto", height: 100 }}
      onWheel={() => { received++; }}>
      <Card role="listitem">
        <h3>Title</h3>
        <p>Subtitle</p>
      </Card>
    </div>,
  );
  try {
    const card = host.querySelector('[role="listitem"]')!;
    for (const target of [card, card.querySelector("h3")!, card.querySelector("p")!]) {
      const event = new window.WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: 40 });
      await act(async () => { target.dispatchEvent(event); });
      assert(!event.defaultPrevented, "card content must leave native wheel scrolling enabled");
    }
    assert(received === 3, "each wheel must reach the list exactly once");
  } finally {
    await act(async () => { root.unmount(); });
    host.remove();
  }
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
