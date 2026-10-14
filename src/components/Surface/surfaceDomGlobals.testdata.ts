import { Window } from "happy-dom";

type ResizeCallback = (entries: Array<{ contentRect: { width: number; height: number } }>) => void;

export class FakeResizeObserver {
  static instances = new Set<FakeResizeObserver>();
  constructor(private readonly callback: ResizeCallback) {}
  private target: Element | null = null;
  observe(target: Element) { this.target = target; FakeResizeObserver.instances.add(this); }
  unobserve() { FakeResizeObserver.instances.delete(this); }
  disconnect() { FakeResizeObserver.instances.delete(this); }
  emit(width: number, height: number) {
    const rect = new DOMRect(0, 0, width, height);
    if (this.target) {
      this.target.getBoundingClientRect = () => rect;
      for (const canvas of this.target.querySelectorAll("canvas")) canvas.getBoundingClientRect = () => rect;
    }
    this.callback([{ contentRect: { width, height } }]);
  }
}

export const frameQueue = new Map<number, FrameRequestCallback>();
let nextFrame = 1;

export function setupSurfaceDom() {
  const window = new Window({ url: "http://localhost/surface" });
  const define = (name: string, value: unknown) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
  define("IS_REACT_ACT_ENVIRONMENT", true);
  define("window", window);
  define("document", window.document);
  define("navigator", window.navigator);
  for (const name of ["DOMRect", "HTMLElement", "HTMLInputElement", "HTMLTextAreaElement", "HTMLSelectElement", "HTMLCanvasElement", "SVGElement", "Element", "Node", "Event", "KeyboardEvent", "MouseEvent", "WheelEvent", "PointerEvent", "FocusEvent", "InputEvent"]) {
    define(name, (window as unknown as Record<string, unknown>)[name]);
  }
  define("ResizeObserver", FakeResizeObserver);
  define("requestAnimationFrame", (callback: FrameRequestCallback) => {
    const id = nextFrame++;
    frameQueue.set(id, callback);
    return id;
  });
  define("cancelAnimationFrame", (id: number) => { frameQueue.delete(id); });
}

// react-dom picks its input-event strategy when it loads, so the DOM must exist before any test imports it.
setupSurfaceDom();
