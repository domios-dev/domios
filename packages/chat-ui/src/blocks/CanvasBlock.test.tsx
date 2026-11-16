import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { browser, configureHtmlFileLoader, configureLocalArtifactLoader, LocalArtifactScope, mount, RichMarkdown } from "../dsBlocks.testdata";
import { CANVAS_LIVE_FRAME_BUDGET } from "./canvasBlockModel";

browser.HTMLElement.prototype.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: 800, bottom: 320, width: 800, height: 320, toJSON() {} }) as DOMRect;
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = () => undefined;

const canvasTag = (document: unknown) => `<ds-canvas>${JSON.stringify(document)}</ds-canvas>`;
const mixed = canvasTag({
  title: "Header options",
  items: [
    { id: "shot", kind: "image", src: "/shots/after.png", x: 0, y: 0, w: 640, h: 400 },
    { id: "mock", kind: "html", html: "<div class='p-4'>Mock B</div>", x: 680, y: 0, w: 480, h: 400 },
    { id: "gone", kind: "image", src: "/shots/missing.png", x: 0, y: 440, w: 320, h: 200 },
    { id: "note", kind: "note", markdown: "**A** is calmer. <ds-html><b>nested</b></ds-html>", x: 360, y: 440, w: 320, h: 120 },
  ],
  views: [{ name: "compare", x: 0, y: 0, w: 1160, h: 400 }, { name: "notes", x: 0, y: 440, w: 680, h: 200 }],
});
const frames = (root: ParentNode) => Array.from(root.querySelectorAll<HTMLIFrameElement>("iframe[data-testid='chat-canvas-frame']"));
const cspOf = (frame: Element | null | undefined) => /http-equiv="Content-Security-Policy" content="([^"]*)"/.exec(frame?.getAttribute("srcdoc") ?? "")?.[1];
const viewer = () => document.querySelector<HTMLElement>("[data-testid='chat-canvas-viewer']");
const press = (key: string) => act(async () => { browser.dispatchEvent(new browser.KeyboardEvent("keydown", { key, code: key })); });
const click = (element: Element | null | undefined) => act(async () => { (element as HTMLElement).click(); });
const settle = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

function loadFiles() {
  configureLocalArtifactLoader(async (path) => {
    if (path === "/shots/missing.png") throw new Error("Preview path is not referenced");
    return { data: "data:image/png;base64,AA==", media_type: "image/png" };
  });
}

test("a canvas frame keeps the ds-html sandbox attributes and CSP byte for byte", async () => {
  const view = await mount();
  loadFiles();
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-html><b>plain</b></ds-html>\n\n${mixed}`} /></LocalArtifactScope>);
    await settle();
    const htmlFrame = view.host.querySelector("iframe[data-testid='chat-html-preview-frame']");
    const [canvasFrame] = frames(view.host);
    assert.ok(canvasFrame, "the inline preview runs the visible HTML item");
    assert.equal(canvasFrame.getAttribute("sandbox"), "allow-scripts");
    assert.equal(canvasFrame.getAttribute("referrerpolicy"), "no-referrer");
    assert.ok(cspOf(htmlFrame)?.startsWith("default-src 'none'"));
    assert.equal(cspOf(canvasFrame), cspOf(htmlFrame));
    assert.ok(canvasFrame.getAttribute("srcdoc")?.includes("Mock B"));
    assert.equal(canvasFrame.style.pointerEvents, "none", "the inline preview never takes input");
    assert.equal(frames(view.host).length, 1, "a note never runs nested ds-html");
  } finally { await view.close(); configureLocalArtifactLoader(null); }
});

test("the inline preview is a static opener, a failed item says one quiet sentence, and bad JSON explains itself", async () => {
  const view = await mount();
  loadFiles();
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`${mixed}\n\n<ds-canvas>{"items":[}</ds-canvas>`} /></LocalArtifactScope>);
    await settle();
    const opener = view.host.querySelector("[role='button'][aria-label^='Open canvas Header options']");
    assert.ok(opener, "the preview opens the canvas");
    assert.equal(opener.querySelectorAll("button, [data-canvas-select]").length, 0, "the preview has no controls of its own");
    const failure = view.host.querySelector("[data-canvas-item='gone'] [data-canvas-failure]");
    assert.equal(failure?.querySelector("p")?.textContent, "This image is not available.");
    assert.equal(failure?.querySelector("details")?.textContent, "DetailsPreview path is not referenced");
    assert.ok(view.host.querySelector("[data-canvas-item='shot'] img"), "a loaded image renders");
    const invalid = view.host.querySelector("[data-canvas-invalid]");
    assert.equal(invalid?.querySelector("p")?.textContent, "This canvas could not be shown because its description is not valid.");
    assert.match(invalid?.querySelector("details")?.textContent ?? "", /^DetailsThe JSON is invalid: /);
  } finally { await view.close(); configureLocalArtifactLoader(null); }
});

test("the opened canvas pages through views, selects an item, and Escape returns to the canvas before closing", async () => {
  const view = await mount();
  loadFiles();
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={mixed} /></LocalArtifactScope>);
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer();
    assert.ok(stage, "click opens the full canvas");
    assert.equal(frames(view.host).length, 0, "the inline preview stops its frames while the canvas is open");
    const pager = () => stage.querySelector("nav[aria-label='Canvas views'] [aria-live]")?.textContent;
    assert.equal(pager(), "All");
    for (const expected of ["Compare 1 / 2", "Notes 2 / 2", "All"]) {
      await click(stage.querySelector("[aria-label='Next view']"));
      assert.equal(pager(), expected);
    }
    await click(stage.querySelector("[aria-label='Previous view']"));
    assert.equal(pager(), "Notes 2 / 2");
    const mock = () => stage.querySelector("[data-canvas-item='mock']");
    assert.equal(frames(stage)[0]?.style.pointerEvents, "none", "an unselected frame lets the canvas pan");
    await click(mock()?.querySelector("[data-canvas-select]"));
    assert.equal(mock()?.getAttribute("data-selected"), "true");
    assert.equal(frames(stage)[0]?.style.pointerEvents, "auto", "the selected frame receives input");
    await press("Escape");
    assert.ok(viewer(), "Escape first returns the item to the canvas");
    assert.equal(mock()?.getAttribute("data-selected"), null);
    await press("Escape");
    assert.equal(viewer(), null, "a second Escape closes the canvas");
  } finally { await view.close(); configureLocalArtifactLoader(null); configureHtmlFileLoader(null); }
});

test("live HTML frames stay within the measured budget and a selected idle item becomes live", async () => {
  const view = await mount();
  const items = Array.from({ length: CANVAS_LIVE_FRAME_BUDGET + 4 }, (_, index) => ({ id: `h${index}`, kind: "html", html: `<p>Frame ${index}</p>`, title: `Frame ${index}` }));
  try {
    await view.render(<RichMarkdown text={canvasTag({ items })} />);
    assert.equal(frames(view.host).length, CANVAS_LIVE_FRAME_BUDGET, "inline frames respect the budget");
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer()!;
    assert.equal(frames(stage).length, CANVAS_LIVE_FRAME_BUDGET);
    const last = stage.querySelector(`[data-canvas-item='h${items.length - 1}']`);
    assert.equal(last?.querySelector("[data-canvas-frame-idle]")?.textContent, `Frame ${items.length - 1}`, "an idle frame shows its title");
    await click(last?.querySelector("[data-canvas-select]"));
    assert.equal(frames(stage).length, CANVAS_LIVE_FRAME_BUDGET);
    assert.ok(last?.querySelector("iframe"), "selecting an idle frame makes it live within the budget");
  } finally { await view.close(); }
});
