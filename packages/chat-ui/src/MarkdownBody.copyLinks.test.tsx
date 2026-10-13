import assert from "node:assert/strict";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
for (const name of ["window", "document", "navigator", "HTMLElement", "SVGElement", "Element", "Node"] as const) {
  Object.defineProperty(globalThis, name, { value: name === "window" ? window : window[name], configurable: true });
}
window.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) as unknown as MediaQueryList;
const { RichMarkdown } = await import("./MarkdownBody");
let passed = 0;
for (const [markdown, expected] of [
  ["`123456`", "123456"],
  ["```\n123456\n```", "123456"],
  ["```ts\n  const n = 1;  \n\n```", "  const n = 1;  \n"],
  ["```html\n<b>123456</b>\n```", "<b>123456</b>"],
  ["```\n a\n b  \n```", " a\n b  "],
  ["<b>raw HTML</b>\n\n```\n123456\n```", "123456"],
] as const) {
  const calls: string[] = [];
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { calls.push(text); } } });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RichMarkdown text={markdown} />));
    const buttons = host.querySelectorAll<HTMLButtonElement>('button[aria-label="Copy code"]');
    assert.equal(buttons.length, 1, markdown);
    assert.equal(host.querySelector('code button'), null);
    await act(async () => buttons[0].click());
    assert.deepEqual(calls, [expected]);
    passed++;
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
}
const host = document.createElement("div");
const root = createRoot(host);
try {
  await act(async () => root.render(<RichMarkdown text={'https://something.com/x http://example.com www.example.org mailto:owner@example.com something.com sub.domain.dev/x?y=1\n\n`code.dev`\n\n```\nfence.dev\n```\n\n[already.dev](https://target.dev)\n\n<a href="https://raw.dev">inside.dev</a> <code>rawcode.dev</code>'} />));
  assert.deepEqual(Array.from(host.querySelectorAll("a"), (link) => link.getAttribute("href")), ["https://something.com/x", "http://example.com", "http://www.example.org", "mailto:owner@example.com", "https://something.com", "https://sub.domain.dev/x?y=1", "https://target.dev", "https://raw.dev"]);
  assert.equal(host.querySelector("a a, code a"), null);
  passed++;
} finally {
  await act(async () => root.unmount());
}
console.log(`MarkdownBody copy/link wiring: ${passed} passed`);
