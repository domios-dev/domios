import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { RichMarkdown } from "./MarkdownBody";

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

test("streaming text and font scale changes still re-render", async () => {
  setupDom();
  const view = await mount(<RichMarkdown text="# Head" fontScale={0} />);
  const small = view.host.querySelector("h1")?.className;
  await view.render(<RichMarkdown text="# Head" fontScale={3} />);
  assert.notEqual(view.host.querySelector("h1")?.className, small);

  await view.render(<RichMarkdown text={"# Head\n\npartial"} fontScale={3} />);
  assert.match(view.host.textContent ?? "", /partial/);
  await view.render(<RichMarkdown text={"# Head\n\npartial **answer**"} fontScale={3} />);
  assert.equal(view.host.querySelector("strong")?.textContent, "answer");
  await view.unmount();
});
