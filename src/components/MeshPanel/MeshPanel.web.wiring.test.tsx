import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AppConfig } from "../../lib/types";
import { MeshPanel } from "./MeshPanel";
import { webUiSnapshot } from "./webUiFixture.testdata";

vi.mock("./useMeshData", () => ({ useMeshData: () => ({ thisMachine: { name: "Local", ip: "127.0.0.1" }, peers: [], services: [], loading: false, error: null }) }));
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
    const web = host.querySelector<HTMLButtonElement>('button[aria-label="Web"]');
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
