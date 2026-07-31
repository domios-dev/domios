// Run: npx tsx packages/chat-ui/src/MarkdownBody.html.test.tsx

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

const testWindow = setupDom();

const { RichMarkdown } = await import("./MarkdownBody");
const { sanitizeStyleValue, containsHtmlMarkup } = await import("./htmlSanitize");
const { buildHtmlPreviewDocument } = await import("./HtmlPreviewBlock");

test("containsHtmlMarkup gates on real tags, not comparisons", () => {
  assert(containsHtmlMarkup("press <kbd>Ctrl</kbd>"), "opening tag detected");
  assert(containsHtmlMarkup("</div>"), "closing tag detected");
  assert(!containsHtmlMarkup("a < b and c > d"), "bare comparison is not markup");
  assert(!containsHtmlMarkup("2 <3"), "numeric comparison is not markup");
});

test("sanitizeStyleValue keeps cosmetics and drops escape hatches", () => {
  assert(sanitizeStyleValue("color: red; font-weight: bold") === "color: red; font-weight: bold;", "cosmetic declarations survive");
  assert(sanitizeStyleValue("color: color-mix(in srgb, red 40%, blue)") === "color: color-mix(in srgb, red 40%, blue);", "color functions survive");
  assert(sanitizeStyleValue("position: fixed; top: 0") === null, "position and offsets are not allowlisted");
  assert(sanitizeStyleValue("background: url(https://evil.test/x.png)") === null, "url() is stripped");
  assert(sanitizeStyleValue("behavior: url(#x)") === null, "behavior is stripped");
  assert(sanitizeStyleValue("color: expression(alert(1))") === null, "expression() is stripped");
  assert(sanitizeStyleValue("transform: translate(0,-400px)") === null, "transform is not allowlisted");
});

test("sanitizeStyleValue resists comment and escape bypasses", () => {
  assert(sanitizeStyleValue("position/**/: fixed") === null, "comment between name and colon does not smuggle position");
  assert(sanitizeStyleValue("/*x*/color: red") === "color: red;", "leading comment does not break a legit declaration");
  assert(sanitizeStyleValue("\\70 osition: fixed") === null, "css escape in the property name is rejected");
  assert(sanitizeStyleValue("background: \\75 rl(//evil.test)") === null, "css escape in the value is rejected");
  assert(sanitizeStyleValue("color: red; z-index: 9999") === "color: red;", "unlisted property drops, listed one survives");
});

test("inline HTML renders as elements", async () => {
  const { host } = await render(<RichMarkdown text={"press <kbd>Ctrl</kbd> and <mark>read this</mark>"} />);
  assert(host.querySelector("kbd")?.textContent === "Ctrl", "kbd element rendered");
  assert(host.querySelector("mark")?.textContent === "read this", "mark element rendered");
});

test("details/summary renders as a disclosure", async () => {
  const { host } = await render(<RichMarkdown text={"<details><summary>More</summary><p>Hidden</p></details>"} />);
  assert(host.querySelector("details"), "details element rendered");
  assert(host.querySelector("summary")?.textContent === "More", "summary text rendered");
});

test("script tags and event handlers are stripped", async () => {
  const { host } = await render(
    <RichMarkdown text={"<div onclick=\"alert(1)\">hi</div><script>alert(2)</script><img src=x onerror=\"alert(3)\">"} />,
  );
  assert(!host.querySelector("script"), "script element must not render");
  const div = host.querySelector("div[onclick]");
  assert(!div, "inline event handler attribute must be stripped");
  assert(!host.innerHTML.includes("onerror"), "img onerror must be stripped");
  assert(host.textContent?.includes("hi"), "safe content still renders");
});

test("iframe and object tags are stripped from inline HTML", async () => {
  const { host } = await render(<RichMarkdown text={"<iframe src=\"https://evil.test\"></iframe><object data=\"x\"></object>"} />);
  assert(!host.querySelector("iframe"), "inline iframe must not render");
  assert(!host.querySelector("object"), "inline object must not render");
});

test("dangerous inline style declarations are scrubbed but safe ones survive", async () => {
  const { host } = await render(
    <RichMarkdown text={"<span style=\"color: tomato; position: fixed; top: 0\">tinted</span>"} />,
  );
  const span = host.querySelector("span[style]");
  assert(span, "styled span rendered");
  const style = span!.getAttribute("style") ?? "";
  assert(/tomato/.test(style), "color survives");
  assert(!/position/.test(style), "position is scrubbed");
});

test("html code fence renders a locked-down sandboxed frame", async () => {
  const { host } = await render(<RichMarkdown text={"```html\n<b>hello</b>\n<script>1</script>\n```"} />);
  const frame = host.querySelector("iframe[data-testid='chat-html-preview-frame']");
  assert(frame, "preview iframe rendered for an html fence");
  assert(frame!.getAttribute("sandbox") === "allow-scripts", "sandbox must be allow-scripts only");
  const srcdoc = frame!.getAttribute("srcdoc") ?? "";
  assert(srcdoc.includes("<b>hello</b>"), "fence source is embedded verbatim");
  assert(srcdoc.includes("Content-Security-Policy"), "CSP meta is embedded");
});

test("html fullscreen survives a markdown parent rerender", async () => {
  const text = "```html\n<b>stable fullscreen</b>\n```";
  const { host, root } = await render(<RichMarkdown text={text} fontScale={1} />);
  const openButton = host.querySelector<HTMLButtonElement>("button[title='Open in fullscreen']");
  assert(openButton, "fullscreen button rendered");
  await act(async () => {
    openButton.dispatchEvent(new testWindow.MouseEvent("click", { bubbles: true }));
  });
  assert(host.querySelector("button[title='Close fullscreen']"), "fullscreen opened");

  await act(async () => {
    root.render(<RichMarkdown text={text} fontScale={1.25} />);
  });
  assert(host.querySelector("button[title='Close fullscreen']"), "fullscreen survives a parent rerender");
});

test("html fullscreen follows its scroll host bounds after resize", async () => {
  const rect = { top: 10, left: 20, width: 320, height: 240 };
  const { host } = await render(
    <div data-testid="scroll-host" style={{ overflowY: "auto" }}>
      <RichMarkdown text={"```html\n<b>responsive fullscreen</b>\n```"} />
    </div>,
  );
  const scrollHost = host.querySelector<HTMLElement>("[data-testid='scroll-host']");
  assert(scrollHost, "scroll host rendered");
  scrollHost.getBoundingClientRect = () => ({
    ...rect,
    right: rect.left + rect.width,
    bottom: rect.top + rect.height,
    x: rect.left,
    y: rect.top,
    toJSON: () => ({ ...rect }),
  } as DOMRect);
  const openButton = host.querySelector<HTMLButtonElement>("button[title='Open in fullscreen']");
  assert(openButton, "fullscreen button rendered");
  await act(async () => {
    openButton.dispatchEvent(new testWindow.MouseEvent("click", { bubbles: true }));
  });
  const fullscreen = host.querySelector<HTMLElement>("button[title='Close fullscreen']")?.closest<HTMLElement>(".fixed");
  assert(fullscreen?.style.width === "320px", "fullscreen starts at the scroll host width");

  rect.width = 640;
  rect.height = 480;
  await act(async () => {
    testWindow.dispatchEvent(new testWindow.Event("resize"));
  });
  assert(fullscreen.style.width === "640px", "fullscreen width follows the resized host");
  assert(fullscreen.style.height === "480px", "fullscreen height follows the resized host");
});

test("preview document CSP blocks network egress", () => {
  const doc = buildHtmlPreviewDocument("<b>x</b>", {
    bg: "#000",
    fg: "#fff",
    accent: "#08f",
    muted: "#888",
    border: "#333",
  });
  assert(doc.includes("connect-src 'none'"), "connect-src is none");
  assert(doc.includes("default-src 'none'"), "default-src is none");
  assert(doc.includes("form-action 'none'"), "form-action is none");
  assert(!doc.includes("allow-same-origin"), "document never opts into same-origin");
});

test("non-html fences still render as code, not previews", async () => {
  const { host } = await render(<RichMarkdown text={"```ts\nconst a = 1;\n```"} />);
  assert(!host.querySelector("iframe"), "ts fence must not produce an iframe");
  assert(host.querySelector("[data-testid='chat-code-block']"), "ts fence renders a code block");
});

let failures = 0;
for (const [name, fn] of tests) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failures += 1;
    console.error(`  ✗ ${name}: ${(error as Error).message}`);
  }
}
if (failures > 0) {
  console.error(`${failures} of ${tests.length} tests failed`);
  process.exit(1);
}
console.log(`✓ ${tests.length} tests passed`);
