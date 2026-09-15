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
  const toolbar = host.querySelector<HTMLElement>('[data-testid="chat-code-toolbar"]');
  assert(toolbar?.classList.contains("select-none"), "copying rendered code should exclude toolbar chrome");
  const scrollHost = host.querySelector<HTMLElement>(`.${SYNTAX_HIGHLIGHT_ROOT_CLASS}.overflow-x-auto.overflow-y-auto`);
  assert(scrollHost?.style.maxHeight === "500px", "code block should cap height at 500px");
  assert(scrollHost?.classList.contains("w-full"), "code lines should scroll inside a width-constrained host");
  assert(scrollHost?.classList.contains("select-text"), "code source remains selectable");
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
  assert(code?.classList.contains("whitespace-pre"), "ASCII spacing and line breaks are preserved");

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
    if (this.dataset.testid === "mermaid-fullscreen-viewport") {
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

  const dialog = host.querySelector('[role="dialog"]');
  const viewport = host.querySelector<HTMLElement>('[data-testid="mermaid-fullscreen-viewport"]');
  const canvas = host.querySelector<HTMLElement>('[data-testid="mermaid-fullscreen-canvas"]');
  const fit = host.querySelector<HTMLButtonElement>('[aria-label="Fit Mermaid diagram to viewport"]');
  const reset = host.querySelector<HTMLButtonElement>('[aria-label="Reset Mermaid diagram to 100%"]');
  assert(dialog?.getAttribute("aria-label") === "Fullscreen Mermaid diagram", "fullscreen has dialog semantics");
  assert(viewport?.classList.contains("h-full"), "Mermaid viewport fills the available height");
  assert(canvas?.style.width === "400px" && canvas.style.height === "200px", "canvas uses SVG viewBox dimensions");
  assert(canvas?.style.transform.includes("scale(2.42)"), "diagram fits and centers on open");
  assert(fit?.getAttribute("aria-pressed") === "true", "Fit exposes its active state");
  assert(reset, "Reset has an accessible name");

  await act(async () => reset.click());
  assert(canvas.style.transform.includes("scale(1)"), "Reset returns to natural 100% size");
  assert(reset.getAttribute("aria-pressed") === "true", "Reset exposes its active state");
  await act(async () => fit.click());

  viewportWidth = 500;
  viewportHeight = 300;
  await act(async () => resize?.([], {} as ResizeObserver));
  assert(canvas.style.transform.includes("scale(1.17)"), "fitted diagram responds to viewport resize");

  const realFrame = window.requestAnimationFrame;
  window.requestAnimationFrame = (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  };
  const wheel = (init: WheelEventInit, ctrlKey = false) => {
    const event = new window.WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: 250, clientY: 150, ...init });
    // The test DOM drops modifier keys and deltas from WheelEventInit, the way pointer ids are defined below.
    Object.defineProperty(event, "ctrlKey", { value: ctrlKey });
    Object.defineProperty(event, "deltaX", { value: init.deltaX ?? 0 });
    Object.defineProperty(event, "deltaY", { value: init.deltaY ?? 0 });
    Object.defineProperty(event, "deltaMode", { value: 0 });
    Object.defineProperty(event, "clientX", { value: init.clientX ?? 250 });
    Object.defineProperty(event, "clientY", { value: init.clientY ?? 150 });
    return event;
  };
  let zoom!: WheelEvent;
  await act(async () => {
    zoom = wheel({ deltaY: -100 }, true);
    viewport.dispatchEvent(zoom);
  });
  assert(zoom.defaultPrevented, "wheel over the diagram never scrolls the chat underneath");
  assert(fit.getAttribute("aria-pressed") === "false", "pinch zoom enters a custom view");
  assert(!canvas.style.transform.includes("scale(1.17)"), "pinch zoom changes the fitted scale");

  const zoomedScale = /scale\(([^)]+)\)/.exec(canvas.style.transform)?.[1];
  const zoomedTranslate = /translate\(([^)]+)\)/.exec(canvas.style.transform)?.[1];
  await act(async () => viewport.dispatchEvent(wheel({ deltaX: 30, deltaY: 20 })));
  assert(/scale\(([^)]+)\)/.exec(canvas.style.transform)?.[1] === zoomedScale, "a two-finger swipe pans without zooming");
  assert(!canvas.style.transform.includes("NaN"), "zooming never produces a non-finite pan");
  assert(/translate\(([^)]+)\)/.exec(canvas.style.transform)?.[1] !== zoomedTranslate, "a two-finger swipe moves the diagram");
  window.requestAnimationFrame = realFrame;

  const pointer = (type: string, pointerId: number, clientX: number, clientY: number) => {
    const event = new window.MouseEvent(type, { bubbles: true, cancelable: true, clientX, clientY });
    Object.defineProperty(event, "pointerId", { value: pointerId });
    Object.defineProperty(event, "pointerType", { value: "mouse" });
    return event;
  };
  await act(async () => fit.click());
  await act(async () => {
    viewport.dispatchEvent(pointer("pointerdown", 1, 100, 100));
    viewport.dispatchEvent(pointer("pointermove", 1, 140, 130));
  });
  assert(canvas.style.transform.includes("translate(40px, 30px)"), "mouse drag pans the diagram");
  await act(async () => viewport.dispatchEvent(pointer("pointerup", 1, 140, 130)));

  await act(async () => fit.click());
  await act(async () => {
    viewport.dispatchEvent(pointer("pointerdown", 1, 100, 100));
    viewport.dispatchEvent(pointer("pointerdown", 2, 200, 100));
    viewport.dispatchEvent(pointer("pointermove", 2, 250, 100));
  });
  assert(!canvas.style.transform.includes("scale(1.17)"), "two-pointer pinch changes diagram scale");

  await act(async () => root.unmount());
  HTMLElement.prototype.getBoundingClientRect = originalRect;
  HTMLElement.prototype.setPointerCapture = originalSetPointerCapture;
  HTMLElement.prototype.hasPointerCapture = originalHasPointerCapture;
  HTMLElement.prototype.releasePointerCapture = originalReleasePointerCapture;
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
