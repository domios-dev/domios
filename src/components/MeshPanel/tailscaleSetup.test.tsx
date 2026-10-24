import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { suggestsRemoteTailscale, tailnetName, tailscaleReady, tailscaleSetupRequest, tailscaleState, type TailscaleState } from "./tailscaleSetup";
import { TailscaleFooter, TailscaleMachineHint, type CanvasTailscale } from "./TailscaleNodes";
import type { ServeStatus } from "../../lib/webAccess";

const serve = (patch: Partial<ServeStatus> = {}): ServeStatus => ({ installed: true, active: false, ...patch });

describe("Tailscale state on this machine", () => {
  it("reads connected from a Tailscale address and names the tailnet from MagicDNS", () => {
    expect(tailscaleState({ serve: serve({ self_dns: "studio.tail1234.ts.net." }), network: { tailscale_ip: "100.64.0.9" }, loading: false }))
      .toEqual({ kind: "connected", tailnet: "tail1234.ts.net" });
    expect(tailscaleState({ serve: null, network: { tailscale_ip: "100.64.0.9" }, loading: true })).toEqual({ kind: "connected", tailnet: null });
  });

  it("tells a missing install from a stopped or signed-out one", () => {
    expect(tailscaleState({ serve: serve({ installed: false }), network: { tailscale_ip: null }, loading: false })).toEqual({ kind: "not_installed" });
    expect(tailscaleState({ serve: serve(), network: { local_ip: "192.168.1.4" }, loading: false })).toEqual({ kind: "not_connected" });
  });

  it("waits while checking and stays quiet when the probe failed", () => {
    expect(tailscaleState({ serve: null, network: null, loading: true })).toEqual({ kind: "checking" });
    expect(tailscaleState({ serve: null, network: null, loading: false })).toEqual({ kind: "unknown" });
    expect(tailscaleReady({ kind: "unknown" })).toBe(true);
    expect(tailscaleReady({ kind: "not_connected" })).toBe(false);
  });

  it("keeps only a real tailnet suffix", () => {
    expect(tailnetName("studio.ts.net")).toBeNull();
    expect(tailnetName(null)).toBeNull();
  });

  it("points an unreachable machine at its own Tailscale only when this machine is connected", () => {
    expect(suggestsRemoteTailscale(false, { kind: "connected", tailnet: null })).toBe(true);
    expect(suggestsRemoteTailscale(true, { kind: "connected", tailnet: null })).toBe(false);
    expect(suggestsRemoteTailscale(false, { kind: "not_installed" })).toBe(false);
  });

  it("names the unreachable machine in the request and keeps this machine's request general", () => {
    const machine = "machine-token-2b9c";
    expect(tailscaleSetupRequest(machine)).toContain(machine);
    expect(tailscaleSetupRequest(null)).not.toContain(machine);
  });
});

async function render(node: React.ReactNode) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, dispose: async () => { await act(async () => root.unmount()); host.remove(); } };
}

function fixture(state: TailscaleState) {
  const calls = { setup: [] as Array<string | null>, install: 0 };
  const tailscale: CanvasTailscale = { state, launching: false, onSetup: (machine) => { calls.setup.push(machine); }, onInstall: () => { calls.install++; } };
  return { tailscale, calls };
}

const button = (host: HTMLElement, name: string) => [...host.querySelectorAll("button")].find((element) => (element.getAttribute("aria-label") ?? element.textContent) === name);

describe("Tailscale on the Mesh canvas", () => {
  it("hands setup to the General agent and keeps a manual install link when Tailscale is not ready", async () => {
    const { tailscale, calls } = fixture({ kind: "not_installed" });
    const view = await render(<TailscaleFooter tailscale={tailscale} />);
    try {
      await act(async () => button(view.host, "Set up with General agent")?.click());
      await act(async () => button(view.host, "Install Tailscale")?.click());
      expect(calls).toEqual({ setup: [null], install: 1 });
    } finally {
      await view.dispose();
    }
  });

  it("shows only the status when connected", async () => {
    const { tailscale } = fixture({ kind: "connected", tailnet: "tail1234.ts.net" });
    const view = await render(<TailscaleFooter tailscale={tailscale} />);
    try {
      expect(button(view.host, "Set up with General agent")).toBeUndefined();
      expect(view.host.textContent).toContain("tail1234.ts.net");
    } finally {
      await view.dispose();
    }
  });

  it("asks the General agent about an unreachable machine by name", async () => {
    const { tailscale, calls } = fixture({ kind: "connected", tailnet: null });
    const view = await render(<TailscaleMachineHint name="Lenovo" tailscale={tailscale} />);
    try {
      await act(async () => button(view.host, "Ask General agent about Tailscale on Lenovo")?.click());
      expect(calls.setup).toEqual(["Lenovo"]);
    } finally {
      await view.dispose();
    }
  });
});
