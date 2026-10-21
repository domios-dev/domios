import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import React, { act, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { HtmlPreviewBlock } from "./HtmlPreviewBlock";

const window = new Window({ url: "http://localhost/viewer" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "KeyboardEvent", "MouseEvent"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
async function mount(content: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(content));
}
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
});
function Harness() {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  return <div><button type="button" onClick={() => setOpen(true)}>Open viewer</button>{open && <FullscreenOverlay panelRef={panel} label="Preview" onClose={() => setOpen(false)}><iframe title="Preview frame" /><button type="button">Last control</button></FullscreenOverlay>}</div>;
}
async function open() {
  const opener = host.querySelector("button")!;
  await act(async () => { opener.focus(); opener.click(); });
  const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
  assert(dialog);
  assert.equal(dialog.parentElement, document.body);
  assert.equal(host.contains(dialog), false);
  assert.equal(dialog.getAttribute("aria-modal"), "true");
  return { opener, dialog };
}
test("viewer portals above its host, traps focus, closes with X and restores focus", async () => {
  await mount(<Harness />);
  const { opener, dialog } = await open();
  const close = dialog.querySelector<HTMLButtonElement>('[aria-label="Close fullscreen viewer"]')!;
  assert.equal(document.activeElement, close);
  await act(async () => window.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Tab", code: "Tab", shiftKey: true, cancelable: true })));
  assert.equal(document.activeElement?.textContent, "Last control");
  await act(async () => close.click());
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(document.activeElement, opener);
});
test("Escape and backdrop close the viewer without closing its host", async () => {
  await mount(<Harness />);
  const { opener } = await open();
  await act(async () => window.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", code: "Escape", cancelable: true })));
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(document.activeElement, opener);
  const { dialog } = await open();
  await act(async () => dialog.click());
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(document.activeElement, opener);
});
test("only the viewer's own HTML frame may forward Escape", async () => {
  await mount(<Harness />);
  const { dialog } = await open();
  await act(async () => window.dispatchEvent(new window.MessageEvent("message", { data: { source: "ct-html-preview", action: "close" } })));
  assert(document.querySelector('[role="dialog"]'));
  const frame = dialog.querySelector("iframe")!;
  await act(async () => window.dispatchEvent(new window.MessageEvent("message", { source: frame.contentWindow, data: { source: "ct-html-preview", action: "close" } })));
  assert.equal(document.querySelector('[role="dialog"]'), null);
});
test("HTML fullscreen uses the shared viewport viewer and restores its trigger", async () => {
  await mount(<HtmlPreviewBlock text="<h1>Window preview</h1>" />);
  const opener = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Fullscreen")!;
  await act(async () => { opener.focus(); opener.click(); });
  const dialog = document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]')!;
  assert.equal(dialog.parentElement, document.body);
  assert(dialog.querySelector("iframe"));
  await act(async () => dialog.querySelector<HTMLButtonElement>('[aria-label="Close fullscreen viewer"]')!.click());
  assert.equal(document.activeElement, opener);
});
test("closing nested viewers restores page scrolling and the original opener", async () => {
  function NestedHarness() {
    const [outer, setOuter] = useState(false);
    const [inner, setInner] = useState(false);
    const outerRef = useRef<HTMLDivElement>(null);
    const innerRef = useRef<HTMLDivElement>(null);
    return <><button type="button" onClick={() => setOuter(true)}>Open viewer</button>{outer && <FullscreenOverlay panelRef={outerRef} label="Outer" onClose={() => setOuter(false)}><button type="button" onClick={() => setInner(true)}>Open nested</button>{inner && <FullscreenOverlay panelRef={innerRef} label="Inner" onClose={() => setInner(false)}><button type="button" onClick={() => setOuter(false)}>Close both</button></FullscreenOverlay>}</FullscreenOverlay>}</>;
  }
  document.body.style.overflow = "scroll";
  await mount(<NestedHarness />);
  const opener = host.querySelector("button")!;
  await act(async () => { opener.focus(); opener.click(); });
  await act(async () => Array.from(document.querySelectorAll("button")).find(button => button.textContent === "Open nested")!.click());
  assert.equal(document.querySelectorAll('[role="dialog"]').length, 2);
  await act(async () => Array.from(document.querySelectorAll("button")).find(button => button.textContent === "Close both")!.click());
  assert.equal(document.querySelectorAll('[role="dialog"]').length, 0);
  assert.equal(document.body.style.overflow, "scroll");
  assert.equal(document.activeElement, opener);
  document.body.style.overflow = "";
});
test("supported document fullscreen enters and native exit closes the shared viewer", async () => {
  const enabled = Object.getOwnPropertyDescriptor(document, "fullscreenEnabled");
  const element = Object.getOwnPropertyDescriptor(document, "fullscreenElement");
  const request = HTMLElement.prototype.requestFullscreen;
  const exit = document.exitFullscreen;
  let current: Element | null = null;
  let requests = 0;
  Object.defineProperty(document, "fullscreenEnabled", { configurable: true, value: true });
  Object.defineProperty(document, "fullscreenElement", { configurable: true, get: () => current });
  HTMLElement.prototype.requestFullscreen = async function () {
    requests += 1;
    current = this;
    document.dispatchEvent(new window.Event("fullscreenchange"));
  };
  document.exitFullscreen = async () => { current = null; document.dispatchEvent(new window.Event("fullscreenchange")); };
  try {
    await mount(<Harness />);
    const { opener, dialog } = await open();
    assert.equal(requests, 1);
    assert.equal(current, dialog);
    await act(async () => root.render(<Harness />));
    assert.equal(document.querySelector('[role="dialog"]'), dialog);
    await act(async () => document.exitFullscreen());
    assert.equal(document.querySelector('[role="dialog"]'), null);
    assert.equal(document.activeElement, opener);
  } finally {
    HTMLElement.prototype.requestFullscreen = request;
    document.exitFullscreen = exit;
    if (enabled) Object.defineProperty(document, "fullscreenEnabled", enabled); else Reflect.deleteProperty(document, "fullscreenEnabled");
    if (element) Object.defineProperty(document, "fullscreenElement", element); else Reflect.deleteProperty(document, "fullscreenElement");
  }
});
