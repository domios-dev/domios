import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Service } from "./types";
import { MeshInventoryList } from "./MeshInventoryList";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  for (const name of ["HTMLElement", "HTMLInputElement", "HTMLTextAreaElement", "HTMLSelectElement", "ResizeObserver"] as const) {
    if (name === "ResizeObserver") continue;
    Object.defineProperty(globalThis, name, { value: window[name], configurable: true });
  }
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

function resource(overrides: Partial<Service> & Pick<Service, "id" | "name" | "kind">): Service {
  return {
    id: overrides.id,
    name: overrides.name,
    kind: overrides.kind,
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 1,
    updated_at: 1,
    enabled: true,
    ...overrides,
  };
}

const resources: Service[] = [
  resource({
    id: "svc-api",
    name: "API",
    kind: "daemon",
    machine_id: "machine-local",
    machine_name: "Local",
    capabilities: { get: { allowed: true } },
  }),
  resource({
    id: "cron-backup",
    name: "Backup",
    kind: "cron",
    machine_id: "machine-remote",
    machine_name: "Remote",
    schedule: "0 3 * * *",
    capabilities: { get: { allowed: true } },
  }),
];

async function main() {
  setupDom();
  const selected: string[] = [];
  const { host, root } = await render(
    <MeshInventoryList
      resources={resources}
      thisMachineName="Local"
      selectedResourceKey={selected[0] ?? null}
      onSelectResource={(_, key) => selected.push(key)}
      headerActions={<button type="button" data-testid="mesh-header-action">Web UI</button>}
    />,
  );

  const search = host.querySelector<HTMLInputElement>("[data-testid='mesh-inventory-search']");
  const rows = () => Array.from(host.querySelectorAll<HTMLButtonElement>("[data-testid='mesh-inventory-row']"));
  assert(search?.getAttribute("aria-label") === "Search inventory", "search has an accessible name");
  assert(rows().length === 2, "service and schedule render as separate rows");
  assert(rows()[0]?.getAttribute("aria-label")?.includes("machine Local"), "row label includes machine identity");
  assert(host.querySelector("[data-testid='mesh-inventory-sort-name']")?.getAttribute("aria-label") === "Sort by Name", "sort control has an accessible name");
  const headerActions = host.querySelector("[data-testid='mesh-inventory-header-actions']");
  assert(headerActions?.querySelector("[data-testid='mesh-header-action']") != null, "header action renders inside the inventory toolbar seam");
  const scrollRegion = host.querySelector("[data-testid='mesh-inventory-scroll-region']");
  assert(scrollRegion?.className.includes("pb-24") === true, "inventory scroll region reserves bottom bubble clearance");
  assert(scrollRegion?.contains(rows()[rows().length - 1] ?? null) === true, "last inventory row remains reachable in the scroll region");

  await act(async () => {
    window.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Slash", key: "/", bubbles: true }));
  });
  assert(document.activeElement === search, "slash focuses search from outside editable controls");

  await act(async () => rows()[0]?.focus());
  await act(async () => rows()[0]?.dispatchEvent(new window.KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", bubbles: true })));
  assert(document.activeElement === rows()[1], "ArrowDown moves focus to the next row");

  await act(async () => rows()[1]?.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true })));
  assert(selected[0] === "machine-remote:cron-backup", "Enter opens the machine-qualified selected identity");
  await act(async () => rows()[0]?.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Space", key: " ", bubbles: true })));
  assert(selected[1] === "machine-local:svc-api", "Space opens a resource row");

  const searchValueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    if (!search) return;
    searchValueSetter?.call(search, "missing");
    search.dispatchEvent(new window.Event("input", { bubbles: true }));
    search.dispatchEvent(new window.Event("change", { bubbles: true }));
  });
  assert(host.querySelector("[data-testid='mesh-inventory-filtered-empty']") != null, "all rows filtered out uses filtered-empty state");
  assert(host.querySelector("[data-testid='mesh-inventory-announcement']")?.textContent?.includes("hidden by the current filters") === true, "filter removal is announced");

  await act(async () => root.unmount());
  console.log("✓ Mesh inventory list exposes keyboard, filtering, and selected identity semantics");
}

void main();
