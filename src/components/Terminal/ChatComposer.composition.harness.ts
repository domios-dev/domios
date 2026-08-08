// Installs happy-dom globals for ChatComposer.composition.test.tsx. This module
// MUST be imported before react-dom: react-dom freezes its DOM feature
// detection at import time (canUseDOM), and importing it without a document
// leaves the change plugin in a polyfill mode where 'input' events never reach
// onChange and every keydown dispatch throws inside React.
import { Window } from "happy-dom";

const win = new Window({ url: "http://localhost/" });

// Present a Tauri runtime marker so the bridge resolves IS_WEB=false: unmocked
// invokes then fail loudly instead of attempting real HTTP fetches.
(win as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};

Object.defineProperty(globalThis, "window", { value: win, configurable: true });
Object.defineProperty(globalThis, "document", { value: win.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: win.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: win.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "HTMLButtonElement", { value: win.HTMLButtonElement, configurable: true });
Object.defineProperty(globalThis, "HTMLTextAreaElement", { value: win.HTMLTextAreaElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: win.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: win.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: win.Node, configurable: true });
Object.defineProperty(globalThis, "Event", { value: win.Event, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: win.CustomEvent, configurable: true });
Object.defineProperty(globalThis, "KeyboardEvent", { value: win.KeyboardEvent, configurable: true });
Object.defineProperty(globalThis, "InputEvent", { value: win.InputEvent, configurable: true });
Object.defineProperty(globalThis, "requestAnimationFrame", {
  configurable: true,
  value: (callback: FrameRequestCallback) => {
    callback(0);
    return 0;
  },
});
Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });
class StubResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
Object.defineProperty(globalThis, "ResizeObserver", { configurable: true, value: StubResizeObserver });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

export const harnessWindow = win;
