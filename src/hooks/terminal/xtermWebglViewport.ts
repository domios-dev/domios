// `@xterm/addon-webgl` resizes canvas.width/height on device-pixel-content-box without calling gl.viewport,
// shrinking WebGL output into the bottom-left whenever compositor DPR differs from window.devicePixelRatio.

export interface WebglViewportContext {
  readonly VIEWPORT: number;
  readonly drawingBufferWidth?: number;
  readonly drawingBufferHeight?: number;
  isContextLost?: () => boolean;
  getParameter: (pname: number) => unknown;
  viewport: (x: number, y: number, width: number, height: number) => void;
}

export interface WebglCanvasLike {
  readonly width: number;
  readonly height: number;
  getContext: (contextId: "webgl2") => WebglViewportContext | null;
  addEventListener?: (type: string, listener: () => void) => void;
  removeEventListener?: (type: string, listener: () => void) => void;
}

export function webglViewportMatchesBuffer(
  viewport: ArrayLike<number> | null | undefined,
  width: number,
  height: number,
): boolean {
  return Boolean(viewport)
    && viewport!.length >= 4
    && viewport![0] === 0
    && viewport![1] === 0
    && viewport![2] === width
    && viewport![3] === height;
}

export function syncWebglCanvasViewport(
  canvas: WebglCanvasLike,
  refresh: () => void,
): boolean {
  const gl = canvas.getContext("webgl2");
  if (!gl || gl.isContextLost?.()) return false;
  const width = gl.drawingBufferWidth || canvas.width;
  const height = gl.drawingBufferHeight || canvas.height;
  if (width <= 0 || height <= 0) return false;
  const viewport = gl.getParameter(gl.VIEWPORT) as ArrayLike<number> | null | undefined;
  if (webglViewportMatchesBuffer(viewport, width, height)) return false;
  gl.viewport(0, 0, width, height);
  refresh();
  return true;
}

export function findAddedWebglCanvas(
  screen: ParentNode | null,
  prior: ReadonlySet<HTMLCanvasElement>,
): HTMLCanvasElement | null {
  if (!screen) return null;
  const canvases = Array.from(screen.querySelectorAll("canvas")).reverse();
  for (const canvas of canvases) {
    if (prior.has(canvas)) continue;
    try {
      if (canvas.getContext("webgl2")) return canvas;
    } catch {
      // Non-WebGL overlay canvas.
    }
  }
  return null;
}

export function observeXtermWebglViewport(
  canvas: HTMLCanvasElement,
  refresh: () => void,
  ResizeObserverCtor: typeof ResizeObserver | undefined = typeof ResizeObserver === "undefined" ? undefined : ResizeObserver,
): () => void {
  let disposed = false;
  const sync = () => {
    if (!disposed) syncWebglCanvasViewport(canvas, refresh);
  };

  sync();

  let observer: ResizeObserver | null = null;
  if (ResizeObserverCtor) {
    observer = new ResizeObserverCtor(() => {
      sync();
      queueMicrotask(sync);
    });
    try {
      observer.observe(canvas, { box: "device-pixel-content-box" });
    } catch {
      observer.observe(canvas);
    }
  }

  const onContextRestored = () => queueMicrotask(sync);
  canvas.addEventListener("webglcontextrestored", onContextRestored);

  return () => {
    disposed = true;
    observer?.disconnect();
    canvas.removeEventListener("webglcontextrestored", onContextRestored);
  };
}
