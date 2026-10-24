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

test("fullscreen code opens a centred reading view with line numbers and an optional side panel", async () => {
  const open = async (host: HTMLElement) => {
    const fullscreen = Array.from(host.querySelectorAll("button")).find((button) => button.textContent?.includes("Fullscreen"));
    assert(fullscreen, "code exposes fullscreen");
    await act(async () => fullscreen.click());
    const dialog = document.querySelector('[aria-label="Fullscreen code"]');
    assert(dialog, "fullscreen code has dialog semantics");
    return dialog;
  };
  const withAside = await render(<CodeBlock text={"def main(value): return value + 1"} language="python" fullscreenAside={<p>Outline: main(value)</p>} />);
  const dialog = await open(withAside.host);
  const reader = dialog.querySelector('[data-testid="code-reader"]');
  assert(reader?.querySelector(".ct-code-reader__sheet"), "the code sits on a reading sheet");
  assert(reader?.querySelector('[aria-label="About this code"]')?.textContent?.includes("main(value)"), "the side panel describes the code");
  assert(reader?.querySelector(".ct-code-reader__sheet")?.textContent?.includes("1"), "the reading view numbers its lines");
  assert(Array.from(dialog.querySelectorAll("button")).some((button) => button.textContent?.includes("Copy")), "Copy stays");
  await act(async () => { document.dispatchEvent(new (window as unknown as { KeyboardEvent: typeof KeyboardEvent }).KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })); });
  assert(!document.querySelector('[aria-label="Fullscreen code"]'), "Escape closes the reading view");
  await act(async () => withAside.root.unmount());
  withAside.host.remove();
  const chat = await render(<CodeBlock text={"const answer = 42;"} language="typescript" />);
  const chatDialog = await open(chat.host);
  assert(chatDialog.querySelector(".ct-code-reader__sheet"), "chat code gets the same reading sheet");
  assert(!chatDialog.querySelector('[aria-label="About this code"]'), "chat code has no side panel");
  await act(async () => chat.root.unmount());
  chat.host.remove();
});

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

test("RichMarkdown preserves line structure in unlabelled fenced blocks", async () => {
  const markdown = [
    "```",
    "building.......... ███████████████████░  97%",
    "finding bug....... ████░░░░░░░░░░░░░░░░  19%",
    "your motivation... ██░░░░░░░░░░░░░░░░░░   8%",
    "```",
  ].join("\n");

  const { host, root } = await render(<RichMarkdown text={markdown} />);
  const code = host.querySelector<HTMLElement>('[data-testid="chat-plain-code-block"]');

  assert(code, "unlabelled fence renders as a compact block instead of inline code");
  assert(!host.querySelector('[data-testid="chat-code-block"]'), "plain text does not get heavy code chrome");
  assert(code?.textContent?.trimEnd().split("\n").length === 3, "all fenced lines remain separate");

  await act(async () => root.unmount());
});

test("fullscreen Mermaid fits its viewport and exposes accessible view controls", async () => {
  let viewportWidth = 1000;
  let viewportHeight = 600;
  let resize: ResizeObserverCallback | null = null;
  const originalRect = HTMLElement.prototype.getBoundingClientRect;
  const originalSetPointerCapture = HTMLElement.prototype.setPointerCapture;
  const originalHasPointerCapture = HTMLElement.prototype.hasPointerCapture;
  const originalReleasePointerCapture = HTMLElement.prototype.releasePointerCapture;
  HTMLElement.prototype.setPointerCapture = () => undefined;
  HTMLElement.prototype.hasPointerCapture = () => true;
  HTMLElement.prototype.releasePointerCapture = () => undefined;
  HTMLElement.prototype.getBoundingClientRect = function getBoundingClientRect() {
    if (this.getAttribute("aria-label") === "Zoomable canvas") {
      return {
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: viewportWidth,
        bottom: viewportHeight,
        width: viewportWidth,
        height: viewportHeight,
        toJSON: () => ({}),
      } as DOMRect;
    }
    return originalRect.call(this);
  };
  class TestResizeObserver {
    constructor(callback: ResizeObserverCallback) {
      resize = callback;
    }
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  Object.defineProperty(globalThis, "ResizeObserver", { value: TestResizeObserver, configurable: true });
  (window as unknown as {
    mermaid: {
      initialize: () => void;
      render: () => { svg: string };
    };
  }).mermaid = {
    initialize: () => {},
    render: () => ({ svg: '<svg viewBox="0 0 400 200"><text>fit diagram</text></svg>' }),
  };

  const { host, root } = await render(
    <CodeBlock text={"flowchart LR\n  Fit --> Resize"} language="mermaid" />,
  );
  await flushEffects();
  const fullscreen = Array.from(host.querySelectorAll("button"))
    .find((button) => button.textContent?.includes("Fullscreen"));
  assert(fullscreen, "Mermaid block exposes fullscreen");
  await act(async () => fullscreen.click());

  const dialog = document.querySelector('[role="dialog"]');
  const viewport = document.querySelector<HTMLElement>('[aria-label="Zoomable canvas"]')!;
  const canvas = viewport.firstElementChild as HTMLElement;
  const fit = document.querySelector<HTMLButtonElement>('[aria-label="Fit Mermaid diagram to viewport"]')!;
  assert(dialog?.getAttribute("aria-label") === "Fullscreen Mermaid diagram", "fullscreen has dialog semantics");
  assert(canvas.querySelector("svg"), "diagram remains an SVG in the camera layer");
  assert(canvas.style.transform.includes("scale(2.36)"), "diagram fits on open");
  assert(fit.getAttribute("aria-pressed") === "true", "Fit exposes its active state");
  const fitted = canvas.style.transform;
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Zoom in"]')!.click());
  assert(canvas.style.transform !== fitted, "zoom control changes camera");
  assert(fit.getAttribute("aria-pressed") === "false", "zoom exits fit mode");
  assert(document.querySelector('[aria-label="Zoom level"]')?.textContent !== "236%", "zoom readout follows the live camera");
  const reset = document.querySelector<HTMLButtonElement>('[aria-label="Reset Mermaid diagram to 100%"]')!;
  await act(async () => reset.click());
  assert(canvas.style.transform.includes("scale(1)"), "100% reset restores natural size");
  assert(reset.getAttribute("aria-pressed") === "true", "Reset exposes its active state");
  const plainWheel = new window.WheelEvent("wheel", { bubbles: true, cancelable: true, deltaX: 30, deltaY: 20 });
  await act(async () => viewport.dispatchEvent(plainWheel));
  assert(plainWheel.defaultPrevented, "plain scrolling is contained");
  assert(canvas.style.transform.includes("scale(1)"), "two-finger scroll pans without zooming");
  await act(async () => fit.click());
  assert(canvas.style.transform === fitted, "fit restores camera");
  viewportWidth = 500; viewportHeight = 300;
  await act(async () => resize?.([], {} as ResizeObserver));
  assert(canvas.style.transform.includes("scale(1.11)"), "fit follows viewport resize");

  await act(async () => root.unmount());
  HTMLElement.prototype.getBoundingClientRect = originalRect;
  HTMLElement.prototype.setPointerCapture = originalSetPointerCapture;
  HTMLElement.prototype.hasPointerCapture = originalHasPointerCapture;
  HTMLElement.prototype.releasePointerCapture = originalReleasePointerCapture;
});

for (const source of ["const answer = 42;\nconsole.log(answer);\n", `${"x".repeat(200_001)}\nsecond line\n`]) {
  test(`numbered code preserves copied source (${source.length} characters)`, async () => {
    let copied: string | null = null;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (text: string) => { copied = text; } },
    });
    const { host, root } = await render(<CodeBlock text={source} language="typescript" showLineNumbers wrapLines />);
    try {
      const copy = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Copy");
      assert(copy, "copy action is available for numbered source");
      await act(async () => copy.click());
      assert(copied === source, "copy preserves source without line numbers");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}

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
