// Run: npx tsx src/components/Canvas/cards/SectionNode.richContent.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { SectionNode } from "./SectionNode";
import { FIXTURE_PLAN, FIXTURE_SECTION } from "./fixtures.testdata";
import type { Plan } from "../types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const RICH_SECTION_BODY = [
  "# Auth design",
  "",
  "Users **sign in** via OAuth.",
  "",
  "```ts",
  "export const token = 1;",
  "```",
  "",
  "```mermaid",
  "flowchart TD",
  "  Login --> Token",
  "```",
].join("\n");

function planWithRichBody(): Plan {
  return {
    ...FIXTURE_PLAN,
    sections: [{ ...FIXTURE_SECTION, body: RICH_SECTION_BODY, subsections: [] }],
  };
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/canvas-section-rich" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  window.matchMedia = ((() => ({
    matches: false,
    media: "",
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as unknown) as typeof window.matchMedia;
  Object.defineProperty(window.navigator, "clipboard", {
    value: { writeText: async () => {} },
    configurable: true,
  });
  const mermaidCalls: string[] = [];
  (window as unknown as {
    mermaid: { initialize: () => void; render: (id: string, source: string) => { svg: string } };
  }).mermaid = {
    initialize: () => {},
    render: (_id, source) => {
      mermaidCalls.push(source);
      return { svg: '<svg data-testid="mock-mermaid"><text>mock graph</text></svg>' };
    },
  };
  return { mermaidCalls };
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return { host, root };
}

const cardStyle = { position: "absolute" as const, left: 0, top: 0, width: 420, height: 520 };

test("SPEC section body delegates to RichMarkdown for markdown, code, and mermaid at W3", async () => {
  const { host, root } = await render(
    <SectionNode variant="spec" plan={planWithRichBody()} style={cardStyle} world="W3" />,
  );
  const body = host.querySelector('[data-testid="canvas-spec-section-body"]');
  assert(body, "spec section body present at W3");
  assert(body.getAttribute("data-renderer") === "rich-markdown", "body delegates to RichMarkdown");
  assert(body.querySelector("h1")?.textContent === "Auth design", "markdown heading rendered");
  assert(body.textContent?.includes("sign in"), "markdown emphasis flattened to text");
  assert(body.textContent?.includes("export const token = 1;"), "typescript code block rendered");
  assert(body.textContent?.includes("Copy"), "CodeBlock copy affordance rendered");
  assert(body.querySelector('[data-testid="mock-mermaid"]'), "mermaid diagram rendered via RichMarkdown");
  await act(async () => root.unmount());
});

test("PLAN section body delegates to RichMarkdown at W2+ without breaking row layout", async () => {
  const w1 = await render(
    <SectionNode variant="plan" plan={planWithRichBody()} style={cardStyle} world="W1" />,
  );
  assert(!w1.host.querySelector('[data-testid="canvas-plan-section-body"]'), "plan body hidden at W1");
  await act(async () => w1.root.unmount());

  const w3 = await render(
    <SectionNode variant="plan" plan={planWithRichBody()} style={cardStyle} world="W3" />,
  );
  const body = w3.host.querySelector('[data-testid="canvas-plan-section-body"]');
  assert(body, "plan section body present at W3");
  assert(body.getAttribute("data-renderer") === "rich-markdown", "plan body delegates to RichMarkdown");
  assert(body.querySelector("h1")?.textContent === "Auth design", "plan body renders markdown heading");
  assert(body.textContent?.includes("export const token = 1;"), "plan body renders code through RichMarkdown");
  assert(w3.host.querySelectorAll('[data-testid="canvas-plan-row"]').length === 1, "plan row layout unchanged");
  await act(async () => w3.root.unmount());
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
