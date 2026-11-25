import { describe, expect, it, vi } from "vitest";
import {
  findAddedWebglCanvas,
  observeXtermWebglViewport,
  syncWebglCanvasViewport,
  webglViewportMatchesBuffer,
  type WebglCanvasLike,
  type WebglViewportContext,
} from "./xtermWebglViewport";

function createFakeGl(initialViewport: [number, number, number, number], buffer: { width: number; height: number }, lost = false) {
  let viewport = [...initialViewport];
  const calls: Array<[number, number, number, number]> = [];
  const gl: WebglViewportContext = {
    VIEWPORT: 0x0ba2,
    get drawingBufferWidth() { return buffer.width; },
    get drawingBufferHeight() { return buffer.height; },
    isContextLost: () => lost,
    getParameter: (pname: number) => (pname === 0x0ba2 ? new Int32Array(viewport) : null),
    viewport: (x: number, y: number, w: number, h: number) => {
      viewport = [x, y, w, h];
      calls.push([x, y, w, h]);
    },
  };
  return { gl, calls, getViewport: () => viewport };
}

describe("xtermWebglViewport", () => {
  it("detects when the WebGL viewport lags behind the enlarged drawing buffer", () => {
    expect(webglViewportMatchesBuffer(new Int32Array([0, 0, 2400, 1800]), 2400, 1800)).toBe(true);
    expect(webglViewportMatchesBuffer(new Int32Array([0, 0, 800, 600]), 2400, 1800)).toBe(false);
    expect(webglViewportMatchesBuffer(new Int32Array([10, 0, 2400, 1800]), 2400, 1800)).toBe(false);
    expect(webglViewportMatchesBuffer(null, 2400, 1800)).toBe(false);
  });

  it("resyncs gl.viewport and refreshes when buffer size disagrees with gl.viewport", () => {
    const buffer = { width: 2400, height: 1800 };
    const { gl, calls, getViewport } = createFakeGl([0, 0, 800, 600], buffer);
    const canvas: WebglCanvasLike = { width: 2400, height: 1800, getContext: () => gl };
    const refresh = vi.fn();

    expect(syncWebglCanvasViewport(canvas, refresh)).toBe(true);
    expect(calls).toEqual([[0, 0, 2400, 1800]]);
    expect(getViewport()).toEqual([0, 0, 2400, 1800]);
    expect(refresh).toHaveBeenCalledTimes(1);

    // Second call is a no-op once aligned.
    expect(syncWebglCanvasViewport(canvas, refresh)).toBe(false);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("skips sync when context is lost or buffer dimensions are zero", () => {
    const lost = createFakeGl([0, 0, 300, 150], { width: 2400, height: 1800 }, true);
    const refresh = vi.fn();
    expect(syncWebglCanvasViewport({ width: 2400, height: 1800, getContext: () => lost.gl }, refresh)).toBe(false);
    expect(syncWebglCanvasViewport({ width: 0, height: 0, getContext: () => createFakeGl([0, 0, 0, 0], { width: 0, height: 0 }).gl }, refresh)).toBe(false);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("resyncs after device-pixel-content-box ResizeObserver updates canvas dimensions without calling gl.viewport", async () => {
    const buffer = { width: 800, height: 600 };
    const { gl, calls } = createFakeGl([0, 0, 800, 600], buffer);
    const canvas = document.createElement("canvas");
    Object.defineProperty(canvas, "width", { configurable: true, get: () => buffer.width });
    Object.defineProperty(canvas, "height", { configurable: true, get: () => buffer.height });
    canvas.getContext = ((id: string) => (id === "webgl2" ? gl : null)) as typeof canvas.getContext;

    let observedCallback: ResizeObserverCallback | null = null;
    let observedOptions: ResizeObserverOptions | undefined;
    const disconnect = vi.fn();
    class FakeResizeObserver {
      constructor(cb: ResizeObserverCallback) { observedCallback = cb; }
      observe(_target: Element, options?: ResizeObserverOptions) { observedOptions = options; }
      unobserve() {}
      disconnect() { disconnect(); }
    }

    const refresh = vi.fn();
    const cleanup = observeXtermWebglViewport(canvas, refresh, FakeResizeObserver as unknown as typeof ResizeObserver);
    expect(observedOptions).toEqual({ box: "device-pixel-content-box" });
    expect(calls).toEqual([]);

    // Simulate xterm's _setCanvasDevicePixelDimensions enlarging canvas at 3x forced DPR
    // while leaving gl.viewport at the 1x emulated DPR size (800x600).
    buffer.width = 2400;
    buffer.height = 1800;
    observedCallback!([], {} as ResizeObserver);
    expect(calls).toEqual([[0, 0, 2400, 1800]]);
    expect(refresh).toHaveBeenCalledTimes(1);

    // Simulate reverse observer order where xterm updates canvas after our callback in the same turn.
    gl.viewport(0, 0, 1000, 700);
    buffer.width = 1000;
    buffer.height = 700;
    calls.length = 0;
    observedCallback!([], {} as ResizeObserver);
    buffer.width = 3000;
    buffer.height = 2100;
    await Promise.resolve();
    expect(calls).toEqual([[0, 0, 3000, 2100]]);

    // Context restore also re-checks viewport.
    gl.viewport(0, 0, 300, 150);
    calls.length = 0;
    canvas.dispatchEvent(new Event("webglcontextrestored"));
    await Promise.resolve();
    expect(calls).toEqual([[0, 0, 3000, 2100]]);

    cleanup();
    expect(disconnect).toHaveBeenCalledTimes(1);
  });

  it("finds the newly added WebGL2 canvas while ignoring prior and 2D overlay canvases", () => {
    const screen = document.createElement("div");
    const existing = document.createElement("canvas");
    const linkLayer = document.createElement("canvas");
    const webglCanvas = document.createElement("canvas");
    const { gl } = createFakeGl([0, 0, 800, 600], { width: 800, height: 600 });
    existing.getContext = (() => gl) as unknown as typeof existing.getContext;
    linkLayer.getContext = (() => null) as unknown as typeof linkLayer.getContext;
    webglCanvas.getContext = ((id: string) => (id === "webgl2" ? gl : null)) as unknown as typeof webglCanvas.getContext;
    screen.append(existing, linkLayer, webglCanvas);

    expect(findAddedWebglCanvas(screen, new Set([existing]))).toBe(webglCanvas);
    expect(findAddedWebglCanvas(screen, new Set([existing, linkLayer, webglCanvas]))).toBeNull();
  });
});
