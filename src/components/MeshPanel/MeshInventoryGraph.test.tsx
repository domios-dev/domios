import { Window } from "happy-dom";
import type { ReactElement } from "react";
import type { Root } from "react-dom/client";
import type { Service } from "./types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  for (const name of ["Element", "Node", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "HTMLTextAreaElement", "HTMLSelectElement"] as const) {
    Object.defineProperty(globalThis, name, { value: window[name], configurable: true });
  }
}

function service(overrides: Partial<Service> & Pick<Service, "id" | "name" | "kind">): Service {
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

const resources = [
  service({ id: "svc-api", name: "API", kind: "daemon", machine_id: "machine-a", machine_name: "Local" }),
  service({ id: "cron-backup", name: "Backup", kind: "cron", machine_id: "machine-b", machine_name: "Remote" }),
];

async function main() {
  setupDom();
  const [{ act }, { createRoot }, { MeshInventoryGraph }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./MeshInventoryGraph"),
  ]);
  async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(ui));
    return { host, root };
  }

  const selected: string[] = [];
  let escaped = false;
  const rendered = await render(
    <MeshInventoryGraph
      resources={resources}
      thisMachineName="Local"
      selectedResourceKey={null}
      onSelectResource={(_, key) => selected.push(key)}
      onEscape={() => { escaped = true; }}
    />,
  );
  const { host, root } = rendered;
  assert(host.querySelector("[data-testid='mesh-graph-canvas']")?.getAttribute("role") === "img", "graph surface has a labelled visual role");
  assert(host.querySelector("[data-testid='mesh-graph-navigator']")?.getAttribute("aria-label") === "Graph navigator", "graph exposes an accessible parallel navigator");
  const nodes = () => Array.from(host.querySelectorAll<HTMLButtonElement>("[data-mesh-graph-nav-item]"));
  assert(nodes().length === 4, "navigator includes machines and resources");
  await act(async () => nodes()[0]?.focus());
  await act(async () => nodes()[0]?.dispatchEvent(new window.KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", bubbles: true })));
  assert(document.activeElement === nodes()[1], "arrow navigation advances through graph nodes");
  await act(async () => nodes()[1]?.dispatchEvent(new window.KeyboardEvent("keydown", { code: "ArrowDown", key: "ArrowDown", bubbles: true })));
  assert(document.activeElement === nodes()[2], "arrow navigation continues through graph nodes");
  const backup = host.querySelector<HTMLButtonElement>("[data-mesh-graph-nav-item][data-resource-key='machine-b:cron-backup']");
  assert(backup, "navigator exposes the expected machine-qualified resource key");
  await act(async () => backup.focus());
  await act(async () => backup.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true })));
  assert(selected[0] === "machine-b:cron-backup", "Enter opens the exact qualified resource key");
  await act(async () => backup.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true })));
  assert(escaped, "Escape invokes the mode-bubble return callback");
  assert(host.querySelector("[data-testid='mesh-graph-announcement']")?.getAttribute("aria-live") === "polite", "selection changes have a polite live announcement");
  await act(async () => root.unmount());
  console.log("✓ Mesh graph navigator exposes selection, keyboard inspection, and escape semantics");
}

void main();
