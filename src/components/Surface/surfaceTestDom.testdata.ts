import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { FakeResizeObserver, frameQueue, setupSurfaceDom } from "./surfaceDomGlobals.testdata";

export { setupSurfaceDom };

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

function labelledBy(element: Element): string | null {
  const ids = element.getAttribute("aria-labelledby");
  if (!ids) return null;
  return ids.split(/\s+/).map((id) => element.ownerDocument.getElementById(id)?.textContent ?? "").join(" ");
}

function labelFor(element: Element): string | null {
  const id = element.getAttribute("id");
  if (!id) return null;
  const label = [...element.ownerDocument.querySelectorAll("label")].find((candidate) => candidate.getAttribute("for") === id);
  return label?.textContent ?? null;
}

function nameOf(element: Element): string {
  return (element.getAttribute("aria-label") ?? labelledBy(element) ?? labelFor(element) ?? element.textContent ?? "").trim();
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

export async function changeValue(element: HTMLInputElement | HTMLSelectElement, value: string, eventName: "input" | "change") {
  await act(async () => {
    const prototype = Object.getPrototypeOf(element) as object;
    Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(element, value);
    element.dispatchEvent(new Event(eventName, { bubbles: true }));
  });
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
