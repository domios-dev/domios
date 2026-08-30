import { Window } from "happy-dom";
import type { ReactElement } from "react";
import type { Root } from "react-dom/client";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";

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

const peer: PeerStatus = {
  instance_id: "machine-remote",
  name: "Remote",
  ip: "10.0.0.2",
  api_port: 7685,
  online: false,
  access_level: "read_only",
  last_seen: null,
  services_stale: true,
  services_error: "peer did not respond",
  services_loaded: false,
  relay_protocol_version: 1,
};
const services = [
  service({ id: "svc-api", name: "API", kind: "daemon", machine_id: "machine-remote", machine_name: "Remote", source_peer_id: "machine-remote", peer_online: false, stale: true }),
  service({ id: "cron-backup", name: "Backup", kind: "cron", machine_id: "machine-remote", machine_name: "Remote", source_peer_id: "machine-remote", capabilities: { get: { allowed: false, unavailable_reason: "not_authorized" } } }),
];

async function main() {
  setupDom();
  const [{ act }, { createRoot }, { ServiceDrawer }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./ServiceDrawer"),
  ]);
  const origin = document.createElement("button");
  origin.type = "button";
  origin.textContent = "origin";
  document.body.append(origin);
  origin.focus();
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  const opened: string[] = [];
  const render = async (state: "peer" | null) => {
    await act(async () => root.render(<ServiceDrawer state={state ? { kind: "peer", id: "Remote" } : null} services={services} peers={[peer]} thisMachineName="Local" onClose={() => void 0} onOpenResource={(_, key) => opened.push(key)} onActionDone={() => void 0} />));
  };
  await render("peer");
  assert(host.querySelector("[data-testid='mesh-peer-drawer']")?.getAttribute("aria-labelledby") === "mesh-peer-drawer-heading", "peer drawer has a labelled dialog heading");
  assert(host.querySelector("[data-testid='mesh-peer-services-section']")?.textContent?.includes("Services (1)"), "Services is a separate discoverable section");
  assert(host.querySelector("[data-testid='mesh-peer-schedules-section']")?.textContent?.includes("Schedules (1)"), "Schedules is a separate discoverable section");
  assert(host.textContent?.includes("Partial peer fetch failure"), "partial peer failure language is visible");
  assert(host.textContent?.includes("offline · cached"), "offline cached resource language is visible");
  assert(host.textContent?.includes("access denied"), "denied capability language is visible");
  assert(document.activeElement?.id === "mesh-peer-drawer-heading", "opening focuses the labelled drawer heading");
  const schedule = host.querySelector<HTMLButtonElement>("[data-resource-key='machine-remote:cron-backup']");
  await act(async () => schedule?.click());
  assert(opened[0] === "machine-remote:cron-backup", "peer resource row opens the exact resource key");
  await render(null);
  assert(document.activeElement === origin, "closing restores focus to the originating control");
  await act(async () => root.unmount());
  console.log("✓ Peer detail separates resource kinds and restores focus");
}

void main();
