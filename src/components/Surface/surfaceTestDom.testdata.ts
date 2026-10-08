import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";

type ResizeCallback = (entries: Array<{ contentRect: { width: number; height: number } }>) => void;

class FakeResizeObserver {
  static instances = new Set<FakeResizeObserver>();
  constructor(private readonly callback: ResizeCallback) {}
  observe() { FakeResizeObserver.instances.add(this); }
  unobserve() { FakeResizeObserver.instances.delete(this); }
  disconnect() { FakeResizeObserver.instances.delete(this); }
  emit(width: number, height: number) { this.callback([{ contentRect: { width, height } }]); }
}

const frameQueue = new Map<number, FrameRequestCallback>();
let nextFrame = 1;

export function setupSurfaceDom() {
  const window = new Window({ url: "http://localhost/surface" });
  const define = (name: string, value: unknown) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
  define("IS_REACT_ACT_ENVIRONMENT", true);
  define("window", window);
  define("document", window.document);
  define("navigator", window.navigator);
  for (const name of ["HTMLElement", "HTMLInputElement", "HTMLTextAreaElement", "HTMLSelectElement", "HTMLCanvasElement", "SVGElement", "Element", "Node", "Event", "KeyboardEvent", "MouseEvent", "WheelEvent", "PointerEvent", "FocusEvent", "InputEvent"]) {
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

export async function resizeStages(width: number, height: number) {
  await act(async () => {
    for (const observer of FakeResizeObserver.instances) observer.emit(width, height);
    const pending = [...frameQueue.values()];
    frameQueue.clear();
    for (const callback of pending) callback(0);
  });
}

export async function render(element: ReactElement): Promise<{ host: HTMLElement; root: Root; unmount(): Promise<void> }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(element));
  return {
    host,
    root,
    async unmount() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

const IMPLICIT_ROLES: Record<string, string> = { BUTTON: "button", SELECT: "combobox", TEXTAREA: "textbox", SECTION: "region" };

function roleOf(element: Element): string | null {
  const explicit = element.getAttribute("role");
  if (explicit) return explicit;
  if (element.tagName === "INPUT") return (element.getAttribute("type") ?? "text") === "text" ? "textbox" : null;
  return IMPLICIT_ROLES[element.tagName] ?? null;
}

function nameOf(element: Element): string {
  return (element.getAttribute("aria-label") ?? element.textContent ?? "").trim();
}

export function queryByRole(scope: ParentNode, role: string, name?: string): HTMLElement | null {
  for (const element of scope.querySelectorAll("*")) {
    if (roleOf(element) !== role) continue;
    if (name === undefined || nameOf(element) === name) return element as HTMLElement;
  }
  return null;
}

export function getByRole(scope: ParentNode, role: string, name?: string): HTMLElement {
  const element = queryByRole(scope, role, name);
  if (!element) throw new Error(`no ${role}${name === undefined ? "" : ` named "${name}"`}`);
  return element;
}

export async function flushAsync() {
  await act(async () => {
    for (let index = 0; index < 10; index += 1) await Promise.resolve();
  });
}

type TestFn = () => void | Promise<void>;

export function testSuite(title: string) {
  const tests: Array<[string, TestFn]> = [];
  return {
    test(name: string, fn: TestFn) { tests.push([name, fn]); },
    async run() {
      setupSurfaceDom();
      let failed = 0;
      for (const [name, fn] of tests) {
        try {
          await fn();
          console.log(`  ✓ ${name}`);
        } catch (error) {
          failed += 1;
          console.error(`  ✗ ${name}: ${error instanceof Error ? error.stack ?? error.message : error}`);
        }
      }
      if (failed > 0) process.exit(1);
      console.log(`\n✓ all ${tests.length} ${title} tests green`);
    },
  };
}
