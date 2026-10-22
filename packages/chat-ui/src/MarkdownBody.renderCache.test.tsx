import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { RichMarkdown } from "./MarkdownBody";
import { parsedMarkdown } from "./markdownParse";

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "SVGElement", "Element", "Node"] as const) {
    const value = key === "window" ? window : key === "document" ? window.document : key === "navigator" ? window.navigator : window[key];
    Object.defineProperty(globalThis, key, { value, configurable: true });
  }
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
}

async function mount(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return {
    host,
    render: (next: React.ReactElement) => act(async () => root.render(next)),
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

test("a remounted transcript renders the same markdown from the render cache", async () => {
  setupDom();
  const text = "# Title\n\nSome **bold** text with `code` and a [link](https://example.com).\n\n- one\n- two";
  const first = await mount(<RichMarkdown text={text} />);
  const html = first.host.innerHTML;
  assert.match(html, /<strong>bold<\/strong>/);
  await first.unmount();

  const second = await mount(<RichMarkdown text={text} />);
  assert.equal(second.host.innerHTML, html);
  await second.unmount();
});

test("streaming content remains readable across font scale changes", async () => {
  setupDom();
  const view = await mount(<RichMarkdown text="# Head" fontScale={0} />);
  const heading = view.host.querySelector("h1");
  assert.equal(heading?.textContent, "Head");
  await view.render(<RichMarkdown text="# Head" fontScale={3} />);
  assert.equal(view.host.querySelector("h1")?.textContent, "Head");

  await view.render(<RichMarkdown text={"# Head\n\npartial"} fontScale={3} />);
  assert.match(view.host.textContent ?? "", /partial/);
  await view.render(<RichMarkdown text={"# Head\n\npartial **answer**"} fontScale={3} />);
  assert.equal(view.host.querySelector("strong")?.textContent, "answer");
  await view.unmount();
});

test("streamed deltas reconcile the existing markdown DOM instead of remounting it", async () => {
  setupDom();
  const head = "# Stream head\n\nFirst paragraph.\n\n```ts\nconst streamed = 1;\n```\n\n";
  const view = await mount(<RichMarkdown text={`${head}partial`} />);
  const heading = view.host.querySelector("h1");
  const paragraph = view.host.querySelector("p");
  const code = view.host.querySelector("pre, code");
  await view.render(<RichMarkdown text={`${head}partial answer with **more**`} />);
  assert.ok(view.host.querySelector("h1") === heading, "heading element was remounted");
  assert.ok(view.host.querySelector("p") === paragraph, "paragraph element was remounted");
  assert.ok(view.host.querySelector("pre, code") === code, "code element was remounted");
  assert.equal(view.host.querySelector("strong")?.textContent, "more");
  await view.unmount();
});

test("rendering never mutates the shared parsed syntax tree", async () => {
  setupDom();
  const deepFreeze = (value: unknown): void => {
    if (!value || typeof value !== "object" || Object.isFrozen(value)) return;
    Object.freeze(value);
    for (const child of Object.values(value)) deepFreeze(child);
  };
  const text = "Visit frozen-example.com today.\n\n- `inline` item\n\n```js\nconst frozen = true;\n```\n\n| A | B |\n| - | - |\n| see docs.example.org | 2 |";
  deepFreeze(parsedMarkdown(text));
  const view = await mount(<RichMarkdown text={text} />);
  assert.equal(view.host.querySelector("a[href='https://frozen-example.com']")?.textContent, "frozen-example.com");
  assert.match(view.host.textContent ?? "", /const frozen = true;/);
  assert.equal(view.host.querySelectorAll("td a").length, 1);
  await view.unmount();
});
