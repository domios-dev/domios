import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";
import type { MeshDataResult } from "./useMeshData";
import { webUiSnapshot } from "./webUiFixture.testdata";
import { MeshPanel } from "./MeshPanel";

const renders = { machines: 0, webCard: 0 };

const inventory: MeshDataResult = {
  thisMachine: { name: "Local", ip: "127.0.0.1" },
  services: [],
  peers: [
    { instance_id: "laptop", name: "Laptop", ip: "100.64.0.2", api_port: 7686, online: true, access_level: "full", last_seen: null, relay_protocol_version: 2 },
    { instance_id: "desktop", name: "Desktop", ip: "100.64.0.3", api_port: 7686, online: true, access_level: "full", last_seen: null, relay_protocol_version: 2 },
  ],
  loading: false, error: null, loaded: true, refresh: () => {},
};

vi.mock("./useMeshData", () => ({ useMeshData: () => inventory }));
vi.mock("../../lib/bridge", async (original) => ({
  ...await original<typeof import("../../lib/bridge")>(),
  invoke: async (command: string) => command === "get_serve_status"
    ? { installed: true, active: true, snapshot: webUiSnapshot("ready") }
    : { api_port: 7686, local_ip: "192.168.1.2" },
  listen: async () => () => {},
}));
vi.mock("qrcode", () => ({ default: { toDataURL: async () => "data:image/png;base64,fixture" } }));
vi.mock("@codeterm/canvas-camera", async (original) => ({
  ...await original<typeof import("@codeterm/canvas-camera")>(),
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, fitToView: () => {}, zoomBy: () => {} }),
}));
vi.mock("./PeerTerminal", async (original) => {
  const actual = await original<typeof import("./PeerTerminal")>();
  return { PeerTerminal: (props: ComponentProps<typeof actual.PeerTerminal>) => { renders.machines++; return <actual.PeerTerminal {...props} />; } };
});
vi.mock("./WebAccessPanel", async (original) => {
  const actual = await original<typeof import("./WebAccessPanel")>();
  return { ...actual, WebAccessPanel: (props: ComponentProps<typeof actual.WebAccessPanel>) => { renders.webCard++; return <actual.WebAccessPanel {...props} />; } };
});

it("opens a machine drawer in one commit without re-rendering the graph nodes or the Web QR card", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const config = { api: { port: 7686, token: "private-token" }, mesh: { enabled: true } } as AppConfig;
  try {
    await act(async () => root.render(<MeshPanel api={config.api} config={config} onConfigUpdate={async () => {}} onClose={() => {}} />));
    const machine = host.querySelector<HTMLElement>('[role="button"][aria-label="Open Desktop"]');
    expect(machine).not.toBeNull();
    const before = { ...renders };
    expect(before.machines).toBeGreaterThan(0);
    expect(before.webCard).toBeGreaterThan(0);

    act(() => machine!.click());
    expect(document.querySelector('[data-testid="mesh-peer-drawer"]')).not.toBeNull();
    expect(renders).toEqual(before);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
