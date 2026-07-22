import assert from "node:assert/strict";
import { Window } from "happy-dom";

const dom = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: dom, configurable: true });
Object.defineProperty(globalThis, "localStorage", { value: dom.localStorage, configurable: true });
Object.defineProperty(globalThis, "CustomEvent", { value: dom.CustomEvent, configurable: true });
Object.defineProperty(globalThis, "requestAnimationFrame", {
  value: (callback: FrameRequestCallback) => { callback(0); return 1; },
  configurable: true,
});

async function main() {
  const { emitPaneUiMode } = await import("./chatUiMode");
  const events: string[] = [];
  window.addEventListener("codeterm-pane-ui-mode", () => events.push("mode"));
  window.addEventListener("codeterm-pane-resize", (event) => {
    events.push(`resize:${(event as CustomEvent).detail.paneId}`);
  });

  emitPaneUiMode("pane-1", "terminal");
  assert.deepEqual(events, ["mode", "resize:pane-1"]);
  emitPaneUiMode("pane-1", "chat");
  assert.deepEqual(events, ["mode", "resize:pane-1", "mode"]);

  console.log("chat UI mode resize tests: passed");
}

void main();
