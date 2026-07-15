// Run: npx tsx packages/chat-ui/src/CodeBlock.test.tsx

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
  const window = new Window({
    url: "http://localhost/",
  });
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
  Object.defineProperty(window.navigator, "clipboard", {
    value: { writeText: async () => {} },
    configurable: true,
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

async function flushEffects() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

setupDom();

const { CodeBlock } = await import("./CodeBlock");
const { RichMarkdown } = await import("./MarkdownBody");
const { SYNTAX_HIGHLIGHT_ROOT_CLASS } = await import("./syntaxTheme");

test("CodeBlock renders highlighted code structure and copy button", async () => {
  const { host, root } = await render(
    <CodeBlock text={"const answer = 42;\nconsole.log(answer);\n"} language="typescript" />,
  );

  assert(host.textContent?.includes("typescript"), "language label should render");
  assert(host.textContent?.includes("Copy"), "copy button should render");
  assert(host.textContent?.includes("const answer = 42;"), "code text should render");
  const highlightedSpan = Array.from(host.querySelectorAll("span"))
    .find((node) => node.textContent === "const");
  assert(highlightedSpan, "highlighted token span should render");
  assert(host.querySelector("button")?.textContent?.includes("Copy"), "copy action should be a button");
  const block = host.querySelector<HTMLElement>('[data-testid="chat-code-block"]');
  assert(block?.classList.contains("min-w-0"), "code block should not expand a flex parent");
  assert(block?.classList.contains("max-w-full"), "code block should stay within the chat width");
  const scrollHost = host.querySelector<HTMLElement>(`.${SYNTAX_HIGHLIGHT_ROOT_CLASS}.overflow-x-auto.overflow-y-auto`);
  assert(scrollHost?.style.maxHeight === "500px", "code block should cap height at 500px");
  assert(scrollHost?.classList.contains("w-full"), "code lines should scroll inside a width-constrained host");
  const highlightedCode = scrollHost?.querySelector<HTMLElement>("code");
  assert(highlightedCode?.style.whiteSpace === "pre", "code should preserve original line width");
  assert(highlightedCode?.style.wordBreak === "normal", "code should not break long lines");

  await act(async () => root.unmount());
});

test("RichMarkdown renders markdown code blocks and mocked Mermaid output", async () => {
  const mermaidCalls: string[] = [];
  (window as unknown as {
    mermaid: {
      initialize: () => void;
      render: (id: string, source: string) => { svg: string };
    };
  }).mermaid = {
    initialize: () => {},
    render: (id, source) => {
      mermaidCalls.push(source);
      return { svg: `<svg data-testid="mock-mermaid" data-source="${id}"><text>mock graph</text></svg>` };
    },
  };

  const markdown = [
    "# Design note",
    "",
    "```ts",
    "export const value = 1;",
    "```",
    "",
    "```mermaid",
    "flowchart TD",
    "  A --> B",
    "```",
  ].join("\n");

  const { host, root } = await render(<RichMarkdown text={markdown} />);
  await flushEffects();

  assert(host.querySelector("h1")?.textContent === "Design note", "markdown heading should render");
  assert(host.textContent?.includes("export const value = 1;"), "markdown code block should render through CodeBlock");
  assert(host.textContent?.includes("Copy"), "nested CodeBlock copy button should render");
  assert(host.querySelector('[data-testid="mock-mermaid"]'), "mocked mermaid SVG should render");
  assert(mermaidCalls.some((source) => source.includes("flowchart TD")), "mermaid renderer should receive graph source");

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
console.log(`CodeBlock component tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
