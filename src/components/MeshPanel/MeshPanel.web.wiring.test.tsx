import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";
import { MeshPanel } from "./MeshPanel";
import { webUiSnapshot } from "./webUiFixture.testdata";
import { GeneralOverviewRail } from "../General/GeneralWorkspaceOverview";
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

it("opens the Web panel from its graph node with the live Serve status and target", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  try {
    await act(async () => root.render(<MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />));
    const web = host.querySelector<HTMLButtonElement>('button[aria-label="Mesh"]');
    expect(web).not.toBeNull();
    await act(async () => web!.click());
    expect(host.querySelector('[role="dialog"][aria-labelledby="web-access-heading"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="web-access-status"]')?.textContent).toBe("Ready");
    expect(host.querySelector<HTMLInputElement>('input[aria-label="Web address"]')?.value).toBe("https://host.tail.ts.net/");
    expect(web!.getAttribute("aria-pressed")).toBe("true");
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
      <GeneralOverviewRail workspaces={workspaces} teams={teams} tabs={tabs} inventory={inventory} onNavigate={(workspaceId, tabId) => navigated.push([workspaceId, tabId])} />
    </>));
    const mesh = host.querySelector('section[aria-label="Mesh resource source"]')!;
    expect(mesh.textContent).not.toContain(daemon.name);
    expect(mesh.textContent).not.toContain(schedule.name);
    for (const resource of [daemon, schedule]) {
      expect(host.querySelector(`button[aria-label="Open ${resource.name}, machine ${resource.machine_name}"]`)).not.toBeNull();
    }
    const utility = host.querySelector<HTMLButtonElement>(`button[aria-label="Open utility shell ${shell.title}"]`);
    expect(utility).not.toBeNull();
    await act(async () => utility!.click());
    expect(navigated).toEqual([[workspaces[0].id, shell.id]]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
