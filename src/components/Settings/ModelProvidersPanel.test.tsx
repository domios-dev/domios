import assert from "node:assert/strict";
import { test } from "node:test";
import { Window } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ModelProvidersPanel } from "./ModelProvidersPanel";
import type { ModelProviderApi } from "./modelProviderApi";

test("provider controls dispatch once and render the connection result", async () => {
  const window = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const calls: string[] = [];
  let rows = [{ id: "fixture", kind: "openai-compatible" as const, baseUrl: "https://example.test", models: ["old"] }];
  const api: ModelProviderApi = {
    list: async () => rows,
    add: async () => { calls.push("add"); },
    test: async (name) => { calls.push(`test:${name}`); return ["discovered"]; },
    remove: async (name) => { calls.push(`remove:${name}`); rows = []; },
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ModelProvidersPanel api={api} />));
    assert.ok(host.querySelector('[role="region"], section[aria-label="Model providers"]'));
    const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
    await act(async () => button("Test fixture").click());
    assert.equal(host.querySelector('[role="status"]')?.textContent, "discovered");
    await act(async () => button("Remove fixture").click());
    assert.equal(button("Remove fixture"), null);
    await act(async () => host.querySelector("form")!.dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }) as unknown as Event));
    assert.deepEqual(calls, ["test:fixture", "remove:fixture", "add"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    await window.happyDOM.close();
  }
});
