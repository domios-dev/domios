import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { SyntaxHighlighter, loadAllGrammars } from "./syntaxHighlighter";

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function render(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, unmount: () => act(async () => root.unmount()) };
}

test("eager grammars and their aliases highlight on the first render", async () => {
  setupDom();
  for (const language of ["tsx", "rust", "html", "sh"]) {
    const view = await render(<SyntaxHighlighter language={language} useInlineStyles={false}>{"const x = <div>1</div>; fn main() {}"}</SyntaxHighlighter>);
    assert.ok(view.host.querySelector(".token"), `${language} tokenized synchronously`);
    await view.unmount();
  }
});

test("a grammar outside the eager set highlights once the full set loads", async () => {
  setupDom();
  const view = await render(<SyntaxHighlighter language="haskell" useInlineStyles={false}>{"main :: IO ()\nmain = putStrLn \"hi\""}</SyntaxHighlighter>);
  await act(async () => { assert.equal(await loadAllGrammars(), "loaded"); });
  assert.ok(view.host.querySelector(".token"), "haskell tokenized after the lazy grammar load");
  assert.match(view.host.textContent ?? "", /putStrLn/);
  await view.unmount();
});
