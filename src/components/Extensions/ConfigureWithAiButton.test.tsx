import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { CatalogEntry } from "./pluginCatalog";
import { ConfigureWithAiButton } from "./ConfigureWithAiButton";

function entry(configHelp?: string): CatalogEntry {
  return {
    id: "demo",
    displayName: "Demo",
    installed: true,
    inMarketplace: true,
    system: false,
    verified: true,
    removable: true,
    dev: false,
    compatible: true,
    updateAvailable: false,
    configHelp,
  };
}

const optedIn = renderToStaticMarkup(<ConfigureWithAiButton entry={entry("Choose a server.")} />);
if (!optedIn.includes("Configure with AI")) throw new Error("explicit configHelp should enable AI configuration");

const zeroConfig = renderToStaticMarkup(<ConfigureWithAiButton entry={entry()} />);
if (zeroConfig !== "") throw new Error("plugins without configHelp should not render AI configuration");

console.log("ConfigureWithAiButton tests: 2/2 passed");

async function clickConfigAction() {
  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "localStorage", "sessionStorage", "HTMLElement", "Element", "Node", "Event", "MouseEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const { __setInvokeForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "get_config") return { api: { host: "127.0.0.1", port: 7686 } } as T;
    throw new Error(`unexpected invoke: ${command}`);
  });
  const requests: Array<{ url: string; body: Record<string, unknown> }> = [];
  const focusEvents: Array<{ tabId?: string; workspaceId?: string }> = [];
  win.addEventListener("codeterm:focus-tab", (event) => focusEvents.push((event as unknown as CustomEvent<{ tabId?: string; workspaceId?: string }>).detail));
  const g = globalThis as unknown as Record<string, unknown>;
  g.fetch = async (url: string, init: { body?: string }) => {
    requests.push({ url, body: JSON.parse(init.body ?? "{}") });
    return { ok: true, status: 200, json: async () => ({ id: "header-helper", workspace_id: "workspace-general", delivery_state: "launched" }) };
  };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ConfigureWithAiButton entry={entry("Choose a server.")} />));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Configure Demo with AI"]');
    assert.ok(button, "plugin header action has a plugin-specific accessible name");
    await act(async () => button.click());
    await act(async () => { for (let tick = 0; tick < 30 && !requests.length; tick++) await Promise.resolve(); });
    assert.equal(requests.length, 1);
    assert.deepEqual(Object.keys(requests[0].body).sort(), ["description", "task", "title"]);
    assert.equal(requests[0].body.title, "Configure");
    assert.equal(requests[0].body.description, "Configure Demo");
    assert.deepEqual(focusEvents, [{ tabId: "header-helper", workspaceId: "workspace-general" }]);
    console.log("ConfigureWithAiButton posts the shared request and focuses the returned General tab");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
}

void clickConfigAction().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
