import assert from "node:assert/strict";
import { test } from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ZoomCanvas } from "./ZoomCanvas";
import { ImageFullscreen } from "./ImageFullscreen";
import { useCanvasViewport, type UseCanvasViewportResult } from "@codeterm/canvas-camera";

const window = new Window();
for (const key of ["window", "document", "HTMLElement", "Element", "Node", "MouseEvent", "ResizeObserver"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
window.HTMLElement.prototype.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: 800, bottom: 600, width: 800, height: 600, toJSON() {} });
const frames: FrameRequestCallback[] = [];
globalThis.requestAnimationFrame = callback => { frames.push(callback); return frames.length; };
globalThis.cancelAnimationFrame = () => {};

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
  await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Show all"]')!.click());
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
  await act(async () => { frames.shift()?.(0); frames.shift()?.(500); });
  assert.notEqual(layer.style.transform, fitted);
  assert(document.querySelector('[aria-label="Save b"]'));
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="Show all"]')!.click());
  assert.equal(layer.style.transform, fitted);
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="View b"]')!.click());
  await act(async () => { frames.shift()?.(600); frames.shift()?.(1100); });
  assert.notEqual(layer.style.transform, fitted);
  await act(async () => root.unmount()); host.remove();
});

test("camera fly paints every frame without committing React until arrival", async () => {
  frames.length = 0;
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
  await act(async () => { frames.shift()!(0); frames.shift()!(50); });
  assert.equal(renders, initialRenders);
  assert.equal(paints, 2);
  assert.deepEqual(api!.transformRef.current, { x: 70, y: -35, scale: 2.75 });
  await act(async () => frames.shift()!(100));
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
