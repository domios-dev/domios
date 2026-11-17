import { formatLayerTransform } from "@codeterm/canvas-camera";
import assert from "node:assert/strict";
import { act } from "react";

export function measureCanvas() {
  const prototype = window.HTMLElement.prototype;
  const width = Object.getOwnPropertyDescriptor(prototype, "clientWidth");
  const height = Object.getOwnPropertyDescriptor(prototype, "clientHeight");
  const originalObserver = globalThis.ResizeObserver;
  let size = { width: 960, height: 640 };
  const observers = new Set<() => void>();
  Object.defineProperty(prototype, "clientWidth", { configurable: true, get: () => size.width });
  Object.defineProperty(prototype, "clientHeight", { configurable: true, get: () => size.height });
  Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: class {
    constructor(private callback: ResizeObserverCallback) {}
    observe() { observers.add(this.deliver); }
    unobserve() {}
    disconnect() { observers.delete(this.deliver); }
    deliver = () => this.callback([{ contentRect: size } as ResizeObserverEntry], this);
  } });
  return {
    resize: () => act(async () => { assert(observers.size > 0, "the mounted stage owns a resize observer"); size = { width: 700, height: 500 }; for (const deliver of observers) deliver(); }),
    restore() {
      if (width) Object.defineProperty(prototype, "clientWidth", width); else Reflect.deleteProperty(prototype, "clientWidth");
      if (height) Object.defineProperty(prototype, "clientHeight", height); else Reflect.deleteProperty(prototype, "clientHeight");
      Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: originalObserver });
    },
  };
}

export async function checkStageGestures(region: HTMLElement, layer: HTMLElement, resize: () => Promise<void>) {
  const view = region.ownerDocument.defaultView!;
  const fitted = layer.style.transform;
  assert.match(fitted, /scale\([\d.e-]+\)/, "the measured surface has a fitted camera");
  assert.notEqual(fitted, formatLayerTransform({ x: 0, y: 0, scale: 1 }), "initial fit moves the camera off its uninitialized transform");
  region.focus();
  await act(async () => { region.dispatchEvent(new view.KeyboardEvent("keydown", { code: "Equal", bubbles: true, cancelable: true })); });
  assert.notEqual(layer.style.transform, fitted, "zoom keys reach the surface camera");
  const enlarged = layer.style.transform;
  await act(async () => { region.dispatchEvent(new view.KeyboardEvent("keydown", { code: "Minus", bubbles: true, cancelable: true })); });
  assert.notEqual(layer.style.transform, enlarged, "minus zooms out through the same map");
  const zoomed = layer.style.transform;
  const modified = new view.KeyboardEvent("keydown", { code: "Equal", ctrlKey: true, bubbles: true, cancelable: true });
  await act(async () => { region.dispatchEvent(modified); });
  assert.equal(layer.style.transform, zoomed, "modified keys belong to the application");
  assert.equal(modified.defaultPrevented, false);
  const input = region.appendChild(region.ownerDocument.createElement("input"));
  await act(async () => { input.dispatchEvent(new view.KeyboardEvent("keydown", { code: "Equal", bubbles: true, cancelable: true })); });
  assert.equal(layer.style.transform, zoomed, "descendant inputs keep their keys");
  input.remove();
  const touch = (type: string, x: number) => {
    const event = new view.Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "touches", { value: [{ clientX: x, clientY: 100 }] });
    region.dispatchEvent(event);
  };
  await act(async () => { touch("touchstart", 100); touch("touchmove", 140); });
  assert.notEqual(layer.style.transform, zoomed, "one-finger touch pans the surface camera");
  const moved = layer.style.transform;
  await resize();
  assert.equal(layer.style.transform, moved, "resize preserves the user's exact camera");
}
