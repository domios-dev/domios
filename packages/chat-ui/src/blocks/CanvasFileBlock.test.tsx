import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { browser, configureLocalArtifactLoader, LocalArtifactScope, mount, RichMarkdown } from "../dsBlocks.testdata";
import { configureCanvasFilePort, type CanvasWriteResult } from "./canvasFilePort";

browser.HTMLElement.prototype.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: 800, bottom: 320, width: 800, height: 320, toJSON() {} }) as DOMRect;
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = () => undefined;

const SRC = "/work/board.canvas.json";
const board = (note: string) => JSON.stringify({ format: "domios.canvas/1", title: "Board", items: [
  { id: "shot", kind: "image", src: "/work/shot.png", x: 0, y: 0, w: 640, h: 400 },
  { id: "n", kind: "note", markdown: note, x: 0, y: 440, w: 320, h: 120 },
], views: [{ name: "notes", x: 0, y: 440, w: 320, h: 120 }] });
const settle = () => act(async () => { for (let index = 0; index < 6; index++) await Promise.resolve(); });
const click = (element: Element | null | undefined) => act(async () => { (element as HTMLElement).click(); });
const press = (key: string) => act(async () => { browser.dispatchEvent(new browser.KeyboardEvent("keydown", { key, code: key })); });
const viewer = () => document.querySelector<HTMLElement>("[data-testid='chat-canvas-viewer']");

function host(writable: boolean, { text = board("first"), watching = true } = {}) {
  const files = { text, reads: [] as { path: string; kind: string; canvas?: string }[], writes: 0, change: null as null | (() => void), released: 0 };
  configureLocalArtifactLoader(async (path, kind, _tabId, canvas) => {
    files.reads.push({ path, kind, ...(canvas ? { canvas } : {}) });
    if (path === SRC) return { data: files.text, media_type: "application/json" };
    return { data: "data:image/png;base64,AA==", media_type: "image/png" };
  });
  configureCanvasFilePort({
    async write(path, _tabId, base, next): Promise<CanvasWriteResult> {
      assert.equal(path, SRC);
      if (base !== files.text) return { status: "conflict", current: files.text };
      files.text = next;
      files.writes++;
      return { status: "written" };
    },
    async watch(path, _tabId, onChange) {
      assert.equal(path, SRC);
      files.change = onChange;
      return { writable, watching, release: () => { files.released++; } };
    },
  });
  return files;
}
const reset = () => { configureLocalArtifactLoader(null); configureCanvasFilePort(null); };

test("failed canvas items recover on board changes without remounting and keep an explicit retry", async () => {
  const view = await mount();
  const files = host(false);
  let available = false;
  let finish: (() => void) | undefined;
  let reads = 0;
  configureLocalArtifactLoader(async (path, kind, tabId, canvas) => {
    if (path === SRC) return { data: files.text, media_type: "application/json" };
    assert.deepEqual({ path, kind, tabId, canvas }, { path: "/work/shot.png", kind: "image", tabId: "owner", canvas: SRC });
    reads++;
    if (!available) throw new Error("Fixture file access denied");
    await new Promise<void>((resolve) => { finish = resolve; });
    return { data: "data:image/png;base64,AA==", media_type: "image/png" };
  });
  try {
    const render = () => view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-canvas src="${SRC}" />`} /></LocalArtifactScope>);
    await render();
    await settle();
    const item = view.host.querySelector("[data-canvas-item='shot']")!;
    assert.match(item.textContent ?? "", /Fixture file access denied/);
    await render();
    assert.equal(reads, 1, "rendering alone never retries I/O");
    await act(async () => { files.change?.(); });
    await settle();
    assert.equal(reads, 2, "one authoritative board change retries a failed item once");
    const retry = item.querySelector<HTMLButtonElement>("button[aria-label='Retry shot.png']");
    assert.ok(retry, "continuing failure remains actionable");
    await click(retry);
    await settle();
    assert.equal(reads, 3);
    available = true;
    await act(async () => { files.change?.(); files.change?.(); files.change?.(); });
    assert.equal(reads, 4, "changes coalesce while the item read is in flight");
    await act(async () => { finish!(); });
    await settle();
    assert.equal(view.host.querySelector("[data-canvas-item='shot']"), item);
    assert.equal(item.querySelector("img")?.getAttribute("src"), "data:image/png;base64,AA==");
    await act(async () => { files.change?.(); });
    await settle();
    assert.equal(reads, 4, "successful items are not re-read on board changes");
  } finally { await view.close(); reset(); }
  assert.equal(files.released, 1);
  files.change?.();
  assert.equal(reads, 4, "disposed item subscriptions cannot read");
});

test("a retry from an old tab cannot overwrite or read through the new tab", async () => {
  const view = await mount();
  const files = host(false);
  const reads: string[] = [];
  let finish: (() => void) | undefined;
  configureLocalArtifactLoader(async (path, _kind, tabId, canvas) => {
    if (path === SRC) return { data: files.text, media_type: "application/json" };
    assert.equal(canvas, SRC);
    reads.push(tabId!);
    if (tabId === "owner") {
      if (reads.length === 1) throw new Error("Not ready");
      await new Promise<void>((resolve) => { finish = resolve; });
    }
    return { data: `data:image/png;base64,${tabId === "owner" ? "AA==" : "BB=="}`, media_type: "image/png" };
  });
  try {
    const render = (tabId: string) => view.render(<LocalArtifactScope tabId={tabId}><RichMarkdown text={`<ds-canvas src="${SRC}" />`} /></LocalArtifactScope>);
    await render("owner");
    await settle();
    const item = view.host.querySelector("[data-canvas-item='shot']")!;
    const retry = item.querySelector<HTMLButtonElement>("button[aria-label='Retry shot.png']")!;
    assert.ok(retry);
    await act(async () => { retry.click(); retry.click(); });
    assert.deepEqual(reads, ["owner", "owner"], "repeated clicks share the in-flight read");
    await render("other");
    await settle();
    const current = view.host.querySelector("[data-canvas-item='shot']")!;
    assert.equal(current.querySelector("img")?.getAttribute("src"), "data:image/png;base64,BB==");
    await act(async () => { finish!(); retry.click(); });
    await settle();
    assert.equal(current.querySelector("img")?.getAttribute("src"), "data:image/png;base64,BB==", "stale completion cannot replace the current tab's content");
    assert.deepEqual(reads, ["owner", "owner", "other"], "a detached retry cannot issue an old-tab read");
  } finally { await view.close(); reset(); }
  assert.equal(files.released, 2);
});

test("a canvas file renders through the preview loader, reads items through it, and reloads in place", async () => {
  const view = await mount();
  const files = host(false);
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-canvas src="${SRC}" view="notes" />`} /></LocalArtifactScope>);
    await settle();
    assert.deepEqual(files.reads.find((read) => read.path === SRC), { path: SRC, kind: "canvas" });
    assert.deepEqual(files.reads.find((read) => read.path === "/work/shot.png"), { path: "/work/shot.png", kind: "image", canvas: SRC }, "items are authorized through their canvas file");
    const image = view.host.querySelector("[data-canvas-item='shot']");
    assert.ok(view.host.querySelector("[aria-label^='Open canvas Board']"));
    files.text = board("changed by an agent");
    await act(async () => { files.change?.(); });
    await settle();
    assert.equal(view.host.querySelector("[data-canvas-item='shot']"), image, "an unchanged item keeps its element");
    assert.match(view.host.querySelector("[data-canvas-item='n']")?.textContent ?? "", /changed by an agent/);
    assert.equal(files.reads.filter((read) => read.path === "/work/shot.png").length, 1, "a reload does not refetch unchanged items");
    files.text = "{";
    await act(async () => { files.change?.(); });
    await settle();
    assert.equal(view.host.querySelector("[data-canvas-invalid] p")?.textContent, "This canvas file has an error, so this is its last good version.");
    assert.match(view.host.querySelector("[data-canvas-invalid] details")?.textContent ?? "", /^DetailsThe JSON is invalid/);
    assert.ok(view.host.querySelector("[data-canvas-item='shot']"), "the last good board stays beside the error");
    assert.ok(view.host.querySelector("[data-canvas-view-clip]"), "a card on a named view clips to that view");
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer()!;
    assert.equal(stage.querySelector("[data-canvas-strip='notice'] p")?.textContent, "This canvas file has an error, so this is its last good version.", "the opened board says why it cannot be edited");
    assert.equal(stage.querySelector("[aria-label='Add note']"), null, "a read-only canvas has no edit affordances");
    await click(stage.querySelector("[data-canvas-item='n'] [data-canvas-select]"));
    assert.equal(stage.querySelector("[data-canvas-resize]"), null);
  } finally { await view.close(); reset(); }
  assert.equal(files.released, 1, "closing releases the watch");
});

test("the owner deletes and undoes in the viewer and every change writes the file", async () => {
  const view = await mount();
  const files = host(true);
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-canvas src="${SRC}" />`} /></LocalArtifactScope>);
    await settle();
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer()!;
    assert.ok(stage.querySelector("[aria-label='Add note']"));
    await click(stage.querySelector("[data-canvas-item='shot'] [data-canvas-select]"));
    assert.ok(stage.querySelector("[data-canvas-item='shot'] [data-canvas-resize]"), "the selected item shows its resize handle");
    const layer = (id: string) => stage.querySelector<HTMLElement>(`[data-canvas-item='${id}']`)?.style.zIndex;
    assert.deepEqual([layer("shot"), layer("n")], ["1", ""], "the selected item draws above its neighbours");
    await press("Delete");
    await settle();
    assert.equal(stage.querySelector("[data-canvas-item='shot']"), null);
    assert.deepEqual((JSON.parse(files.text) as { items: { id: string }[] }).items.map((item) => item.id), ["n"]);
    await act(async () => { browser.dispatchEvent(new browser.KeyboardEvent("keydown", { key: "z", code: "KeyZ", ctrlKey: true })); });
    await settle();
    assert.ok(stage.querySelector("[data-canvas-item='shot']"), "undo brings the item back");
    assert.deepEqual((JSON.parse(files.text) as { items: { id: string }[] }).items.map((item) => item.id), ["shot", "n"]);
    assert.equal(files.writes, 2);
  } finally { await view.close(); reset(); }
});

const pointer = (target: EventTarget | null | undefined, type: string, clientX: number) => act(async () => {
  (target as EventTarget).dispatchEvent(new browser.PointerEvent(type, { bubbles: true, cancelable: true, button: 0, clientX, clientY: 100 }));
});
const shotX = (text: string) => (JSON.parse(text) as { items: { id: string; x?: number }[] }).items.find((item) => item.id === "shot")?.x;

test("a drag commits the moved frame on release, and a cancelled drag writes nothing but lets held updates land", async () => {
  const view = await mount();
  const files = host(true);
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-canvas src="${SRC}" />`} /></LocalArtifactScope>);
    await settle();
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer()!;
    const shot = () => stage.querySelector<HTMLElement>("[data-canvas-item='shot']");
    await pointer(shot()?.querySelector("[data-canvas-select]"), "pointerdown", 100);
    await pointer(browser, "pointermove", 200);
    assert.equal(shot()?.style.left, "80px", "the item follows the pointer at the 800/640 screen scale");
    assert.equal(shot()?.style.zIndex, "1", "a dragged item draws above its neighbours");
    await pointer(browser, "pointerup", 200);
    await settle();
    assert.equal(shotX(files.text), 80, "the release writes the move");
    assert.equal(files.writes, 1);
    await pointer(shot()?.querySelector("[data-canvas-select]"), "pointerdown", 100);
    await pointer(browser, "pointermove", 300);
    files.text = files.text.replace("first", "agent wrote this");
    await act(async () => { files.change?.(); });
    await settle();
    assert.doesNotMatch(stage.querySelector("[data-canvas-item='n']")?.textContent ?? "", /agent wrote this/, "nothing changes under the pointer");
    await pointer(browser, "pointercancel", 300);
    await settle();
    assert.equal(shot()?.style.left, "80px", "a cancelled drag puts the item back");
    assert.match(stage.querySelector("[data-canvas-item='n']")?.textContent ?? "", /agent wrote this/, "the held update lands when the gesture ends");
    assert.equal(files.writes, 1, "a cancelled drag writes nothing");
  } finally { await view.close(); reset(); }
});

test("the pager follows its target by identity and clamps when the target is deleted", async () => {
  const view = await mount();
  const text = JSON.stringify({ format: "domios.canvas/1", items: [
    { id: "shot", kind: "image", src: "/work/shot.png", x: 0, y: 0, w: 640, h: 400 },
    { id: "n", kind: "note", markdown: "a", title: "First note", x: 0, y: 440, w: 320, h: 120 },
    { id: "m", kind: "note", markdown: "b", title: "Second note", x: 400, y: 440, w: 320, h: 120 },
  ] });
  const files = host(true, { text });
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-canvas src="${SRC}" />`} /></LocalArtifactScope>);
    await settle();
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer()!;
    const pager = () => stage.querySelector("nav[aria-label='Canvas items'] [aria-live]")?.textContent;
    await click(stage.querySelector("[aria-label='Previous item']"));
    assert.equal(pager(), "Second note 3 / 3");
    await click(stage.querySelector("[data-canvas-item='m'] [data-canvas-select]"));
    await press("Delete");
    await settle();
    assert.deepEqual((JSON.parse(files.text) as { items: { id: string }[] }).items.map((item) => item.id), ["shot", "n"]);
    assert.equal(pager(), "First note 2 / 2", "a deleted target clamps to the nearest one");
    files.text = JSON.stringify({ format: "domios.canvas/1", items: [
      { id: "n", kind: "note", markdown: "a", title: "First note", x: 0, y: 440, w: 320, h: 120 },
      { id: "shot", kind: "image", src: "/work/shot.png", x: 0, y: 0, w: 640, h: 400 },
    ] });
    await act(async () => { files.change?.(); });
    await settle();
    assert.equal(pager(), "First note 1 / 2", "an agent's reorder keeps the same target");
  } finally { await view.close(); reset(); }
});

test("a canvas that gets no live-update slot shows its board with a quiet note under Details", async () => {
  const view = await mount();
  const files = host(true, { watching: false });
  try {
    await view.render(<LocalArtifactScope tabId="owner"><RichMarkdown text={`<ds-canvas src="${SRC}" />`} /></LocalArtifactScope>);
    await settle();
    assert.ok(view.host.querySelector("[data-canvas-item='shot']"), "the board still shows");
    assert.match(view.host.querySelector("[data-canvas-details]")?.textContent ?? "", /^DetailsLive updates are off/);
  } finally { await view.close(); reset(); }
  assert.equal(files.released, 1);
});
