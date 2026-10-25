import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";
import { MeshPanel } from "./MeshPanel";
import { webUiSnapshot } from "./webUiFixture.testdata";
import { GeneralInventorySections } from "../Sidebar/GeneralInventory";
import { daemon, schedule, shell, tabs, teams, workspaces } from "../General/generalOverview.testdata";
import type { MeshDataResult } from "./useMeshData";
import { ServiceDrawer } from "./ServiceDrawer";

const inventory: MeshDataResult = {
  thisMachine: { name: "Local", ip: "127.0.0.1" },
  services: [daemon, schedule],
  peers: [{ instance_id: "remote", name: "Remote machine", ip: "100.64.0.1", api_port: 7686, online: false, access_level: "full", last_seen: null, relay_protocol_version: 2 }],
  loading: false, error: "inventory request failed", loaded: true, refresh: () => {},
};
let meshData = inventory;
const refreshKeys: number[] = [];
const registryRefreshKeys: number[] = [];

vi.mock("./useMeshData", () => ({ useMeshData: (refreshKey: number) => { refreshKeys.push(refreshKey); return meshData; } }));
vi.mock("../Jobs/JobsView", () => ({ JobsView: () => <section aria-label="Jobs" /> }));
vi.mock("./useCanvasAccount", () => ({ useCanvasAccount: (_peers: unknown, _changed: unknown, key: number) => { registryRefreshKeys.push(key); return undefined; } }));
vi.mock("./useCanvasTailscale", () => ({ useCanvasTailscale: () => undefined }));
vi.mock("../../lib/bridge", async (original) => ({
  ...await original<typeof import("../../lib/bridge")>(),
  invoke: async (command: string) => command === "get_serve_status"
    ? { installed: true, active: true, snapshot: webUiSnapshot("ready") }
    : { api_port: 7686, local_ip: "192.168.1.2", web_listener: "100.64.1.2:7686" },
  listen: async () => () => {},
}));
vi.mock("qrcode", () => ({ default: { toDataURL: async () => "data:image/png;base64,fixture" } }));
vi.mock("@codeterm/canvas-camera", async (original) => ({
  ...await original<typeof import("@codeterm/canvas-camera")>(),
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, fitToView: () => {}, zoomBy: () => {} }),
}));

it("wires the toolbar Refresh to the account registry as well as inventory", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  try {
    await act(async () => root.render(<MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />));
    const inventoryKey = refreshKeys[refreshKeys.length - 1];
    const registryKey = registryRefreshKeys[registryRefreshKeys.length - 1];
    await act(async () => host.querySelector<HTMLButtonElement>('button[title="Refresh"]')!.click());
    expect(refreshKeys[refreshKeys.length - 1]).toBe(inventoryKey + 1);
    expect(registryRefreshKeys[registryRefreshKeys.length - 1]).toBe(registryKey + 1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("shows the inventory as unavailable instead of an empty graph when the first load fails", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  meshData = {
    thisMachine: { name: "this", ip: "127.0.0.1" }, services: [], peers: [],
    loading: false, error: "request timed out", loaded: false, refresh: () => {},
  };
  try {
    await act(async () => root.render(<MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />));
    const view = host.querySelector('section[aria-label="Mesh inventory"]');
    expect(view?.textContent).toContain("Mesh data is unavailable.");
    expect(host.querySelector('[role="status"]')?.textContent).toBe("Mesh data is unavailable.");
    expect(host.querySelector('[role="region"][aria-label="Mesh UI"]')).toBeNull();
    expect(host.textContent).not.toContain("0 peers");
    expect(host.textContent).not.toContain("127.0.0.1");

    const before = refreshKeys[refreshKeys.length - 1];
    const retry = Array.from(view!.querySelectorAll("button")).find((button) => button.textContent?.includes("Retry"));
    await act(async () => retry!.click());
    expect(refreshKeys[refreshKeys.length - 1]).toBe(before + 1);

    meshData = { ...inventory, error: null };
    await act(async () => retry!.click());
    expect(host.querySelector('section[aria-label="Mesh inventory"]')).toBeNull();
    expect(host.querySelector('[role="region"][aria-label="Mesh UI"]')).not.toBeNull();
    expect(host.textContent).toContain("1 peer");
  } finally {
    meshData = inventory;
    await act(async () => root.unmount());
    host.remove();
  }
});

it("renders a loading header and inventory until cached or live data arrives, never 0 peers", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  const panel = () => <MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />;
  meshData = {
    thisMachine: { name: "this", ip: "127.0.0.1" }, services: [], peers: [],
    loading: true, error: null, loaded: false, refresh: () => {},
  };
  try {
    await act(async () => root.render(panel()));
    expect(host.textContent).not.toContain("0 peers");
    expect(host.querySelector('[aria-busy="true"]')?.textContent).toBe("Loading peers");
    expect(host.querySelector('section[aria-label="Mesh inventory"][aria-busy="true"]')).not.toBeNull();
    expect(host.textContent).not.toContain("127.0.0.1");

    const second = { ...inventory.peers[0], instance_id: "second", name: "Second machine" };
    meshData = { ...inventory, peers: [...inventory.peers, second], error: null, loading: true };
    await act(async () => root.render(panel()));
    expect(host.textContent).toContain("2 peers");
    expect(host.querySelector('section[aria-label="Mesh inventory"]')).toBeNull();
    expect(host.querySelector('[role="region"][aria-label="Mesh UI"]')).not.toBeNull();
  } finally {
    meshData = inventory;
    await act(async () => root.unmount());
    host.remove();
  }
});

it("dismisses the peer drawer before closing Mesh on the next Escape", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  function Harness() {
    const [open, setOpen] = useState(true);
    return open ? <MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => setOpen(false)} /> : null;
  }
  try {
    await act(async () => root.render(<Harness />));
    await act(async () => host.querySelector<HTMLElement>('[role="button"][aria-label="Open Remote machine"]')!.click());
    const drawer = host.querySelector('[role="dialog"]');
    expect(drawer).not.toBeNull();
    expect(drawer!.contains(document.activeElement)).toBe(true);
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true })));
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(host.querySelector('[role="tablist"][aria-label="Mesh sections"]')).not.toBeNull();
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true })));
    expect(host.childElementCount).toBe(0);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("shows the access card directly in the graph without a duplicate details dialog", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  try {
    await act(async () => root.render(<MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />));
    const web = Array.from(host.querySelectorAll('button')).find((button) => button.textContent === 'Web details');
    expect(host.querySelector('[role="region"][aria-label="Mesh UI"] img')).not.toBeNull();
    expect(web).toBeUndefined();
    expect(host.querySelector('section[role="status"][aria-label]')).toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(host.querySelector('[data-testid="web-access-status"]')).toBeNull();
    expect(host.querySelector('[role="region"][aria-label="Mesh UI"]')?.textContent).toContain('100.64.1.2:7686');
    const expand = host.querySelector<HTMLButtonElement>('button[aria-label="Expand Mesh UI"]');
    expect(expand).not.toBeNull();
    await act(async () => expand!.click());
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    await act(async () => document.querySelector<HTMLButtonElement>('button[aria-label="Collapse Mesh UI"]')!.click());
    expect(document.querySelector('[role="dialog"]')).toBeNull();

  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("renders authoritative daemon and schedule data in the map and peer sections", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  const navigated: string[][] = [];
  try {
    await act(async () => root.render(<>
      <section aria-label="Mesh resource source">
        <MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />
        <ServiceDrawer state={{ kind: "peer", id: "Remote machine" }} services={inventory.services} peers={inventory.peers} thisMachineName="Local" onClose={() => {}} onActionDone={() => {}} />
      </section>
      <GeneralInventorySections generalWorkspaceId={workspaces[0].id} workspaces={workspaces} teams={teams} tabs={tabs} inventory={inventory} jobs={[]} onCloseTab={() => {}} onRenameTab={() => {}} onNavigate={(workspaceId, tabId) => navigated.push([workspaceId, tabId])} />
    </>));
    const mesh = host.querySelector('section[aria-label="Mesh resource source"]')!;
    expect(mesh.textContent).toContain(daemon.name);
    expect(mesh.textContent).toContain(schedule.name);
    expect(host.querySelector('section[aria-label="Jobs"]')).toBeNull();
    expect(host.querySelector('section[aria-label="Services"] button[aria-expanded]')).toBeNull();
    for (const name of ["Utility shells"]) {
      const toggle = host.querySelector<HTMLButtonElement>(`section[aria-label="${name}"] button[aria-expanded]`)!;
      if (toggle.getAttribute("aria-expanded") !== "true") await act(async () => toggle.click());
    }
    for (const resource of [daemon, schedule]) {
      expect(host.querySelector(`button[aria-label="Open ${resource.name}, machine ${resource.machine_name}"]`)).toBeNull();
    }
    const utility = host.querySelector<HTMLButtonElement>(`button[aria-label="Open tab qa shell, ${shell.title}"]`);
    expect(utility).not.toBeNull();
    await act(async () => utility!.click());
    expect(navigated).toEqual([[workspaces[0].id, shell.id]]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
