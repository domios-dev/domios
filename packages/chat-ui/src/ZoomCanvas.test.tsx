import assert from "node:assert/strict";
import { test } from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ZoomCanvas } from "./ZoomCanvas";
import { ImageFullscreen } from "./ImageFullscreen";
import { computeFitTransform, formatLayerTransform, useCanvasViewport, type UseCanvasViewportResult } from "@codeterm/canvas-camera";

const window = new Window();
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "MouseEvent", "ResizeObserver"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.HTMLElement.prototype.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: 800, bottom: 600, width: 800, height: 600, toJSON() {} });
const frames = new Map<number, FrameRequestCallback>();
let frameId = 0;
globalThis.requestAnimationFrame = callback => { frames.set(++frameId, callback); return frameId; };
globalThis.cancelAnimationFrame = id => { frames.delete(id); };
function runFrame(time: number) {
  const next = frames.entries().next().value;
  assert(next, "an animation frame is pending");
  frames.delete(next[0]); next[1](time);
}

test("canvas wheel, pan, touch and fit controls manipulate the camera and prevent scrolling", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<ZoomCanvas bounds={{ x: 0, y: 0, width: 400, height: 300 }}><span>World</span></ZoomCanvas>));
  const region = host.querySelector<HTMLElement>('[aria-label="Zoomable canvas"]')!;
  const layer = region.firstElementChild as HTMLElement;
  const fitted = layer.style.transform;
  const wheel = new window.WheelEvent("wheel", { deltaY: -60, clientX: 200, clientY: 150, ctrlKey: true, cancelable: true, bubbles: true });
  await act(async () => region.dispatchEvent(wheel));
  assert(wheel.defaultPrevented);
  assert.notEqual(layer.style.transform, fitted);
  await act(async () => region.dispatchEvent(new window.MouseEvent("dblclick", { bubbles: true })));
  assert.equal(layer.style.transform, fitted);
  const touch = (type: string, points: { clientX: number; clientY: number }[]) => {
    const event = new window.Event(type, { bubbles: true, cancelable: true });
    Object.defineProperties(event, { touches: { value: points }, changedTouches: { value: points } });
    region.dispatchEvent(event);
    assert(event.defaultPrevented);
  };
  await act(async () => {
    touch("touchstart", [{ clientX: 100, clientY: 100 }, { clientX: 200, clientY: 100 }]);
    touch("touchmove", [{ clientX: 50, clientY: 100 }, { clientX: 250, clientY: 100 }]);
  });
  assert.notEqual(layer.style.transform, fitted);
  await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Fit all"]')!.click());
  assert.equal(layer.style.transform, fitted);
  await act(async () => {
    region.dispatchEvent(new window.MouseEvent("mousedown", { button: 0, clientX: 10, clientY: 10, bubbles: true }));
  });
  await act(async () => window.dispatchEvent(new window.MouseEvent("mousemove", { clientX: 70, clientY: 50 })));
  assert.notEqual(layer.style.transform, fitted);
  await act(async () => window.dispatchEvent(new window.MouseEvent("mouseup")));
  await act(async () => root.unmount()); host.remove();
});

test("image navigation flies to the next image and show all restores the gallery", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const images = [{ name: "a", src: "data:image/png;base64,AA" }, { name: "b", src: "data:image/png;base64,BB" }];
  await act(async () => root.render(<ImageFullscreen image={images[0]} images={images} anchorRef={{ current: host }} onClose={() => {}} />));
  assert.equal(document.querySelectorAll('[aria-label="Zoomable canvas"] img').length, 2);
  const layer = document.querySelector('[aria-label="Zoomable canvas"]')!.firstElementChild as HTMLElement;
  const fitted = layer.style.transform;
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Next image"]')!.click());
  await act(async () => { runFrame(0); runFrame(500); });
  assert.notEqual(layer.style.transform, fitted);
  assert(document.querySelector('[aria-label="Save b"]'));
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Fit all"]')!.click());
  assert.equal(layer.style.transform, fitted);
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="View b"]')!.click());
  await act(async () => { runFrame(600); runFrame(1100); });
  assert.notEqual(layer.style.transform, fitted);
  await act(async () => root.unmount()); host.remove();
});

test("camera fly paints every frame without committing React until arrival", async () => {
  frames.clear();
  let renders = 0;
  let paints = 0;
  let api: UseCanvasViewportResult;
  function Harness() {
    renders++;
    api = useCanvasViewport({ width: 800, height: 600, onPaint: () => { paints++; } });
    return <div ref={api.containerRef} {...api.handlers}><div ref={api.layerRef} /></div>;
  }
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Harness />));
  const initialRenders = renders;
  await act(async () => api.animateTo({ x: 80, y: -40, scale: 3 }, 100));
  await act(async () => { runFrame(0); runFrame(50); });
  assert.equal(renders, initialRenders);
  assert.equal(paints, 2);
  assert.deepEqual(api!.transformRef.current, { x: 70, y: -35, scale: 2.75 });
  await act(async () => runFrame(100));
  assert.equal(renders, initialRenders + 1);
  assert.deepEqual(api!.transform, { x: 80, y: -40, scale: 3 });
  await act(async () => root.unmount()); host.remove();
});

test("fullscreen retains denied gallery slots without loading or offering their bytes", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const images = [{ name: "allowed", src: "data:image/png;base64,AA" }, { name: "denied", src: "", error: "private" }];
  await act(async () => root.render(<ImageFullscreen image={images[1]} images={images} anchorRef={{ current: host }} onClose={() => {}} />));
  assert.equal(document.querySelectorAll('[aria-label="Zoomable canvas"] img').length, 1);
  assert(document.querySelector('[aria-label="denied not available"]'));
  assert.equal(document.querySelector('[aria-label="Save denied"]'), null);
  await act(async () => root.unmount()); host.remove();
});

test("late image dimensions preserve focus, then preserve a custom camera across layout and resize", async () => {
  frames.clear();
  const originalRect = window.HTMLElement.prototype.getBoundingClientRect;
  const originalObserver = globalThis.ResizeObserver;
  let width = 800, height = 600;
  let resize = () => {};
  window.HTMLElement.prototype.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: width, bottom: height, width, height, toJSON() {} });
  Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class {
    constructor(callback: () => void) { resize = callback; }
    observe() {} disconnect() {}
  } });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const images = [{ name: "a", src: "data:image/png;base64,AA" }, { name: "b", src: "data:image/png;base64,BB" }];
  try {
    await act(async () => root.render(<ImageFullscreen image={images[0]} images={images} anchorRef={{ current: host }} onClose={() => {}} />));
    const layer = document.querySelector('[aria-label="Zoomable canvas"]')!.firstElementChild as HTMLElement;
    const load = (index: number, naturalWidth: number, naturalHeight: number) => {
      const image = layer.querySelectorAll("img")[index];
      Object.defineProperties(image, { naturalWidth: { value: naturalWidth, configurable: true }, naturalHeight: { value: naturalHeight, configurable: true } });
      image.dispatchEvent(new window.Event("load"));
    };
    await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Next image"]')!.click());
    await act(async () => load(0, 1600, 300));
    assert.equal(layer.querySelector<HTMLButtonElement>('[aria-label="View a"]')!.style.width, "1600px", "the load handler must run before asserting camera preservation");
    await act(async () => { runFrame(0); runFrame(500); });
    const expectedFocus = computeFitTransform({ x: 1640, y: 0, width: 800, height: 600 }, 800, 600, { minScale: 0.0001, maxScale: 8 })!;
    assert.equal(layer.style.transform, formatLayerTransform(expectedFocus));
    await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Zoom in"]')!.click());
    const custom = layer.style.transform;
    const updated = [images[0], { ...images[1], src: "late-image.png" }];
    await act(async () => root.render(<ImageFullscreen image={updated[0]} images={updated} anchorRef={{ current: host }} onClose={() => {}} />));
    assert.equal(layer.querySelectorAll("img")[1].getAttribute("src"), "late-image.png");
    await act(async () => load(1, 400, 200));
    assert.equal(layer.querySelector<HTMLButtonElement>('[aria-label="View b"]')!.style.width, "400px");
    assert.equal(layer.style.transform, custom);
    width = 1000; height = 700;
    await act(async () => resize());
    assert.equal(layer.style.transform, custom);
    await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Fit all"]')!.click());
    assert.notEqual(layer.style.transform, custom);
    assert.equal(document.querySelector('[aria-label="Fit all"]')?.getAttribute("aria-pressed"), "true");
  } finally {
    await act(async () => root.unmount()); host.remove();
    window.HTMLElement.prototype.getBoundingClientRect = originalRect;
    Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: originalObserver });
  }
});

test("phone fullscreen opens a selected image at readable scale among 32 slots", async () => {
  frames.clear();
  const originalMatchMedia = window.matchMedia;
  const originalRect = window.HTMLElement.prototype.getBoundingClientRect;
  window.matchMedia = ((query: string) => ({ matches: query === "(max-width: 600px)" })) as typeof window.matchMedia;
  window.HTMLElement.prototype.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: 390, bottom: 600, width: 390, height: 600, toJSON() {} });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const images = Array.from({ length: 32 }, (_, index) => ({ name: String(index), src: `image-${index}.png` }));
  try {
    await act(async () => root.render(<ImageFullscreen image={images[17]} images={images} anchorRef={{ current: host }} onClose={() => {}} />));
    await act(async () => { runFrame(0); runFrame(500); });
    assert(document.querySelector('[aria-label="Save 17"]'));
    assert.equal(document.querySelector('[aria-label="Fit all"]')?.getAttribute("aria-pressed"), "false");
    const layer = document.querySelector('[aria-label="Zoomable canvas"]')!.firstElementChild as HTMLElement;
    assert(Number(/scale\(([^)]+)\)/.exec(layer.style.transform)![1]) > 0.3);
  } finally {
    await act(async () => root.unmount()); host.remove();
    window.matchMedia = originalMatchMedia;
    window.HTMLElement.prototype.getBoundingClientRect = originalRect;
  }
});

test("plain Mermaid wheel pans, modified wheel zooms, and natural reset updates the live readout", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ZoomCanvas wheelBehavior="pan" bounds={{ x: 0, y: 0, width: 400, height: 300 }}><span>World</span></ZoomCanvas>));
    const region = host.querySelector('[aria-label="Zoomable canvas"]')!;
    const layer = region.firstElementChild as HTMLElement;
    const scale = /scale\(([^)]+)\)/.exec(layer.style.transform)![1];
    const before = layer.style.transform;
    await act(async () => region.dispatchEvent(new window.WheelEvent("wheel", { bubbles: true, cancelable: true, deltaX: 30, deltaY: 20 })));
    assert.notEqual(layer.style.transform, before);
    assert.equal(/scale\(([^)]+)\)/.exec(layer.style.transform)![1], scale);
    const modified = new window.WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -100, clientX: 200, clientY: 200 });
    Object.defineProperty(modified, "ctrlKey", { value: true });
    await act(async () => region.dispatchEvent(modified));
    assert.notEqual(/scale\(([^)]+)\)/.exec(layer.style.transform)![1], scale);
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Reset to 100%"]')!.click());
    assert.equal(host.querySelector('[aria-label="Zoom level"]')!.textContent, "100%");
    assert.equal(host.querySelector('[aria-label="Reset to 100%"]')!.getAttribute("aria-pressed"), "true");
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Zoom out"]')!.click());
    assert.equal(host.querySelector('[aria-label="Zoom level"]')!.textContent, "80%");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

test("canvas keyboard controls navigate and zoom with accessible focus", async () => {
  frames.clear();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const images = [{ name: "a", src: "data:image/png;base64,AA" }, { name: "b", src: "data:image/png;base64,BB" }];
  try {
    await act(async () => root.render(<ImageFullscreen image={images[0]} images={images} anchorRef={{ current: host }} onClose={() => {}} />));
    const region = document.querySelector<HTMLElement>('[aria-label="Zoomable canvas"]')!;
    region.focus(); assert.equal(document.activeElement, region);
    const key = (code: string) => region.dispatchEvent(new window.KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }));
    await act(async () => key("ArrowRight"));
    assert(document.querySelector('[aria-label="Save b"]'));
    await act(async () => key("Equal"));
    await act(async () => key("Digit0"));
    assert.equal(document.querySelector('[aria-label="Zoom level"]')!.textContent, "100%");
    await act(async () => key("Home"));
    assert.equal(document.querySelector('[aria-label="Fit all"]')!.getAttribute("aria-pressed"), "true");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

test("touch selection invokes its callback directly and suppresses a subsequent native click", async () => {
  const selections: number[] = [];
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ZoomCanvas bounds={{ x: 0, y: 0, width: 400, height: 300 }} onSelect={index => selections.push(index)}>
      <button type="button" data-zoom-target="0" onClick={() => selections.push(0)}>Select image</button>
    </ZoomCanvas>));
    const target = host.querySelector<HTMLButtonElement>('[data-zoom-target="0"]')!;
    const point = { clientX: 100, clientY: 100 };
    const touch = (type: string, time: number, ended: boolean) => {
      const event = new window.Event(type, { bubbles: true, cancelable: true });
      Object.defineProperties(event, { touches: { value: ended ? [] : [point] }, changedTouches: { value: [point] }, timeStamp: { value: time } });
      target.dispatchEvent(event);
    };
    await act(async () => { touch("touchstart", 1000, false); touch("touchend", 1050, true); });
    assert.deepEqual(selections, [0]);
    const click = new window.MouseEvent("click", { bubbles: true, cancelable: true, detail: 1, clientX: 100, clientY: 100 });
    Object.defineProperty(click, "timeStamp", { value: 1060 });
    await act(async () => target.dispatchEvent(click));
    assert.deepEqual(selections, [0]);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

test("image preview fit, 100% and zoom controls are the shared canvas toolbar", async () => {
  frames.clear();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const images = [{ name: "a", src: "data:image/png;base64,AA" }];
  try {
    await act(async () => root.render(<ImageFullscreen image={images[0]} images={images} anchorRef={{ current: host }} onClose={() => {}} />));
    const toolbar = document.querySelector('[role="toolbar"][data-testid="canvas-toolbar"]')!;
    assert(toolbar, "zoom controls render as the shared canvas toolbar");
    for (const name of ["Fit all", "Reset to 100%", "Zoom out", "Zoom in"]) {
      assert(toolbar.querySelector(`button[aria-label="${name}"]`), `${name} lives in the shared toolbar`);
    }
    assert.equal(document.querySelectorAll('[role="toolbar"] button').length, 4, "no extra bespoke zoom buttons");
    const click = async (name: string) => act(async () => toolbar.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!.click());
    const level = () => document.querySelector('[aria-label="Zoom level"]')!.textContent;
    await click("Reset to 100%");
    assert.equal(level(), "100%");
    assert.equal(toolbar.querySelector('[aria-label="Reset to 100%"]')!.getAttribute("aria-pressed"), "true");
    await click("Zoom in");
    assert.equal(level(), "125%");
    await click("Zoom out");
    assert.equal(level(), "100%");
    await click("Zoom out");
    assert.equal(level(), "80%");
    await click("Fit all");
    assert.equal(toolbar.querySelector('[aria-label="Fit all"]')!.getAttribute("aria-pressed"), "true");
  } finally { await act(async () => root.unmount()); host.remove(); }
});
