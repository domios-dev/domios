import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import React, { act, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FullscreenOverlay } from "./FullscreenOverlay";
import { HtmlPreviewBlock } from "./HtmlPreviewBlock";
import { AnchoredPopover } from "./AnchoredPopover";
import { CodeBlock } from "./CodeBlock";
import { LocalArtifactBlock } from "./LocalArtifactBlock";
import { configureLocalArtifactLoader } from "./localArtifactLoader";

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
  configureLocalArtifactLoader(null);
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
  const frame = host.querySelector("iframe")!;
  assert(frame);
  const frameWindow = frame.contentWindow;
  const opener = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Fullscreen")!;
  await act(async () => { opener.focus(); opener.click(); });
  const dialog = document.querySelector('[role="dialog"][aria-label="Fullscreen HTML page"]')!;
  assert.equal(dialog.closest("dialog")?.open, true);
  assert.equal(document.querySelectorAll('[role="dialog"][aria-modal="true"]').length, 1);
  assert.equal(dialog.querySelector("iframe"), frame);
  assert.equal(document.querySelectorAll("iframe").length, 1);
  assert.equal(frame.contentWindow, frameWindow);
  assert.equal(dialog.querySelectorAll('[aria-label^="Close fullscreen"]').length, 1);
  await act(async () => dialog.querySelector<HTMLButtonElement>('[aria-label="Close fullscreen viewer"]')!.click());
  assert.equal(document.activeElement, opener);
  assert.equal(host.querySelector("iframe"), frame);
  assert.equal(frame.contentWindow, frameWindow);
});

test("persistent viewer retains child state through backdrop and native cancel dismissal", async () => {
  let mounts = 0;
  function Content() {
    const [count, setCount] = useState(() => { mounts += 1; return 0; });
    return <button type="button" onClick={() => setCount(value => value + 1)}>Count {count}</button>;
  }
  function PersistentHarness() {
    const [open, setOpen] = useState(false);
    const panel = useRef<HTMLDivElement>(null);
    return <><button type="button" onClick={() => setOpen(true)}>Open viewer</button>
      <FullscreenOverlay panelRef={panel} open={open} label="Persistent" onClose={() => setOpen(false)}><Content /></FullscreenOverlay></>;
  }
  await mount(<PersistentHarness />);
  const opener = host.querySelector("button")!;
  const content = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Count 0")!;
  await act(async () => content.click());
  for (const dismissal of ["backdrop", "cancel"]) {
    await act(async () => { opener.focus(); opener.click(); });
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    assert.equal(dialog.contains(content), true);
    assert.equal(content.textContent, "Count 1");
    await act(async () => dismissal === "backdrop" ? dialog.click() : dialog.closest("dialog")!.dispatchEvent(new window.Event("cancel", { cancelable: true })));
    assert.equal(document.querySelector('[role="dialog"]'), null);
    assert.equal(document.activeElement, opener);
  }
  assert.equal(mounts, 1);
});

test("expanding a local document reuses its rendered content and never reloads or renders it twice", async () => {
  let loads = 0;
  let renders = 0;
  configureLocalArtifactLoader(async () => { loads += 1; return { data: "document", media_type: "text/markdown" }; });
  const renderMarkdown = (text: string) => { renders += 1; return <article>{text}</article>; };
  await mount(<LocalArtifactBlock directive={{ kind: "document", path: "/document.md" }} renderMarkdown={renderMarkdown} />);
  const content = host.querySelector("article")!;
  assert(content);
  const opener = host.querySelector<HTMLButtonElement>('[aria-label="Fullscreen document.md"]')!;
  await act(async () => { opener.focus(); opener.click(); });
  assert.equal(document.querySelector('[role="dialog"] article'), content);
  assert.equal(document.querySelectorAll("article").length, 1);
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Close fullscreen viewer"]')!.click());
  assert.equal(host.querySelector("article"), content);
  assert.equal(document.activeElement, opener);
  assert.equal(loads, 1);
  assert.equal(renders, 1);
});

test("only an active content viewer owns nested code and anchored menus, and Escape dismisses its menu first", async () => {
  function Menu({ name }: { name: string }) {
    const [open, setOpen] = useState(false);
    const anchor = useRef<HTMLButtonElement>(null);
    return <><button ref={anchor} type="button" onClick={() => setOpen(true)}>{name}</button>
      <AnchoredPopover open={open} anchor={anchor} onClose={() => setOpen(false)}><button type="button" role="menuitem" onClick={() => setOpen(false)}>Action {name}</button></AnchoredPopover></>;
  }
  function Viewer() {
    const [open, setOpen] = useState(false);
    const panel = useRef<HTMLDivElement>(null);
    return <><section aria-label="Ordinary code"><CodeBlock text="outside" /></section><Menu name="Outside menu" />
      <button type="button" onClick={() => setOpen(true)}>Open viewer</button>
      <FullscreenOverlay panelRef={panel} open={open} label="Content" onClose={() => setOpen(false)}><CodeBlock text="inside" /><Menu name="Inside menu" /></FullscreenOverlay></>;
  }
  const find = (text: string) => Array.from(document.querySelectorAll("button")).find(button => button.textContent === text)!;
  await mount(<Viewer />);
  const outsideFullscreen = host.querySelector<HTMLButtonElement>('[aria-label="Ordinary code"] [title="Open in fullscreen"]')!;
  assert.equal(outsideFullscreen.hidden, false);
  await act(async () => find("Outside menu").click());
  assert.equal(document.querySelector("[data-anchored-popover]")?.parentElement, document.body);
  await act(async () => document.body.dispatchEvent(new window.PointerEvent("pointerdown", { bubbles: true })));
  assert.equal(document.querySelector("[data-anchored-popover]"), null);
  const opener = find("Open viewer");
  await act(async () => { opener.focus(); opener.click(); });
  const viewer = document.querySelector('[role="dialog"]')!;
  assert.equal(viewer.querySelector<HTMLButtonElement>('[title="Open in fullscreen"]')?.hidden, true);
  assert.equal(outsideFullscreen.hidden, false);
  const menuTrigger = find("Inside menu");
  await act(async () => { menuTrigger.focus(); menuTrigger.click(); });
  assert.equal(document.querySelector("[data-anchored-popover]")?.parentElement, viewer);
  await act(async () => find("Action Inside menu").focus());
  await act(async () => find("Action Inside menu").dispatchEvent(new window.KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true })));
  assert.equal(document.querySelector("[data-anchored-popover]"), null);
  assert.equal(document.querySelector('[role="dialog"]'), viewer);
  assert.equal(document.activeElement, menuTrigger);
  await act(async () => window.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Escape", key: "Escape", cancelable: true })));
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(document.activeElement, opener);
  assert.equal(host.querySelector<HTMLButtonElement>('dialog [title="Open in fullscreen"]')?.hidden, false);
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
test("the viewer covers the Domios window and never requests document fullscreen", async () => {
  const request = HTMLElement.prototype.requestFullscreen;
  let requests = 0;
  HTMLElement.prototype.requestFullscreen = async function () { requests += 1; };
  try {
    await mount(<Harness />);
    const { opener, dialog } = await open();
    assert.equal(requests, 0);
    await act(async () => root.render(<Harness />));
    assert.equal(document.querySelector('[role="dialog"]'), dialog);
    await act(async () => window.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", code: "Escape", cancelable: true })));
    assert.equal(document.querySelector('[role="dialog"]'), null);
    assert.equal(document.activeElement, opener);
  } finally {
    HTMLElement.prototype.requestFullscreen = request;
  }
});
