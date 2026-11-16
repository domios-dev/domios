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

function host(writable: boolean) {
  const files = { text: board("first"), reads: [] as { path: string; kind: string; canvas?: string }[], writes: 0, change: null as null | (() => void), released: 0 };
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
      return { writable, release: () => { files.released++; } };
    },
  });
  return files;
}
const reset = () => { configureLocalArtifactLoader(null); configureCanvasFilePort(null); };

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
    assert.equal(view.host.querySelector("[data-canvas-invalid] p")?.textContent, "This canvas could not be shown because its description is not valid.");
    assert.ok(view.host.querySelector("[data-canvas-item='shot']"), "the last good board stays beside the error");
    await click(view.host.querySelector("[aria-label^='Open canvas']"));
    const stage = viewer()!;
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
