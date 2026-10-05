// Run: node scripts/run-ts-unit-tests.mjs src/components/Extensions/GlancePopover.configure.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";

async function main() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "sessionStorage", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ __setInvokeForTest }, { GlancePopover }] = await Promise.all([
    import("../../lib/bridge"),
    import("./GlancePopover"),
  ]);
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "plugin_glance") return { title: "Telegram Client", nodes: [] } as T;
    if (command === "get_config") return {} as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  const requests: Array<{ url: string; body: Record<string, unknown> }> = [];
  const focusEvents: Array<{ tabId?: string; workspaceId?: string }> = [];
  win.addEventListener("codeterm:focus-tab", (event) => focusEvents.push((event as unknown as CustomEvent<{ tabId?: string; workspaceId?: string }>).detail));
  const g = globalThis as unknown as Record<string, unknown>;
  g.fetch = async (url: string, init?: { body?: string }) => {
    requests.push({ url: String(url), body: JSON.parse(String(init?.body)) });
    return { ok: true, status: 200, json: async () => ({ id: "helper-glance", workspace_id: "workspace-general", delivery_state: "launched" }) };
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let closed = false;
  try {
    await act(async () => root.render(<GlancePopover
      pluginId="telegram-client"
      displayName="Telegram Client"
      configHelp="Choose a server."
      anchor={{ right: 0 }}
      onClose={() => { closed = true; }}
      onOpenSettings={() => {}}
    />));
    await act(async () => { for (let tick = 0; tick < 20 && !host.textContent?.includes("Configure with AI"); tick++) await Promise.resolve(); });
    const button = Array.from(document.body.querySelectorAll<HTMLButtonElement>("button")).find((item) => item.textContent?.includes("Configure with AI"));
    assert.ok(button, "Glance exposes its AI configure action");
    await act(async () => button.click());
    await act(async () => { for (let tick = 0; tick < 30 && !requests.length; tick++) await Promise.resolve(); });
    assert.equal(requests.length, 1);
    assert.deepEqual(Object.keys(requests[0].body).sort(), ["description", "task", "title"]);
    assert.equal(requests[0].body.title, "Configure");
    assert.equal(requests[0].body.description, "Configure Telegram Client");
    assert.ok(String(requests[0].body.task).includes("telegram-client"));
    assert.deepEqual(focusEvents, [{ tabId: "helper-glance", workspaceId: "workspace-general" }]);
    await act(async () => { for (let tick = 0; tick < 20 && !closed; tick++) await Promise.resolve(); });
    assert.equal(closed, true);
    console.log("Glance Configure with AI sends the shared request and focuses its returned General tab");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
}

void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
