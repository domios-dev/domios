import { act } from "react";
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
  peers: [{ instance_id: "remote", name: "Remote machine", ip: "100.64.0.1", api_port: 7686, online: false, access_level: "full", last_seen: null, relay_protocol_version: 1 }],
  loading: false, error: null, refresh: () => {},
};

vi.mock("./useMeshData", () => ({ useMeshData: () => inventory }));
vi.mock("../Jobs/JobsView", () => ({ JobsView: () => <section aria-label="Jobs" /> }));
vi.mock("../../lib/bridge", async (original) => ({
  ...await original<typeof import("../../lib/bridge")>(),
  invoke: async (command: string) => command === "get_serve_status"
    ? { installed: true, active: true, snapshot: webUiSnapshot("ready") }
    : { api_port: 7686, local_ip: "192.168.1.2" },
  listen: async () => () => {},
}));
vi.mock("qrcode", () => ({ default: { toDataURL: async () => "data:image/png;base64,fixture" } }));
vi.mock("../Canvas/useCanvasViewport", () => ({
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, fitToView: () => {}, zoomBy: () => {} }),
}));

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
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(host.querySelector('[data-testid="web-access-status"]')).toBeNull();
    expect(host.querySelector('[role="region"][aria-label="Mesh UI"]')?.textContent).toContain('host.tail.ts.net');
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

it("renders the same daemon and schedule source in General after removing Mesh resource surfaces", async () => {
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
      <GeneralInventorySections workspaces={workspaces} teams={teams} tabs={tabs} inventory={inventory} jobs={[]} onCloseTab={() => {}} onRenameTab={() => {}} onNavigate={(workspaceId, tabId) => navigated.push([workspaceId, tabId])} />
    </>));
    const mesh = host.querySelector('section[aria-label="Mesh resource source"]')!;
    expect(mesh.textContent).not.toContain(daemon.name);
    expect(mesh.textContent).not.toContain(schedule.name);
    for (const name of ["Jobs", "Services", "Utility shells"]) {
      const toggle = host.querySelector<HTMLButtonElement>(`section[aria-label="${name}"] button[aria-expanded]`)!;
      if (toggle.getAttribute("aria-expanded") !== "true") await act(async () => toggle.click());
    }
    for (const resource of [daemon, schedule]) {
      expect(host.querySelector(`button[aria-label="Open ${resource.name}, machine ${resource.machine_name}"]`)).not.toBeNull();
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
