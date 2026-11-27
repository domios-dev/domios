import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderCliStatus } from "../../lib/providerCli";

import type { PeerEvent } from "../../lib/peerEventRail";

const peerObservers = new Map<string, Set<(event: PeerEvent) => void>>();
vi.mock("../../lib/peerEventRail", () => ({
  peerEventRail: {
    observe(peerId: string, listener: (event: PeerEvent) => void) {
      const listeners = peerObservers.get(peerId) ?? new Set();
      peerObservers.set(peerId, listeners);
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  },
}));

const openUrl = vi.hoisted(() => vi.fn(async () => {}));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { openUrl } }));

const events = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    events.set(name, handler);
    return () => { events.delete(name); };
  }),
}));

import * as webApi from "../../lib/webApi";
import { invoke } from "../../lib/bridge";
import { TabLaunchFailure } from "./TabLaunchFailure";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const MISSING_REASON =
  "Runtime could not start: Provider not installed: Opaque Harness (`opaque-harness`) launches `opaque-bin`, which is not on this machine. Searched: [/usr/bin]. Install command: npm install -g opaque-harness. fix: domios agent cli install opaque-harness";

describe("ProviderNotInstalledNotice via TabLaunchFailure", () => {
  beforeEach(() => {
    events.clear();
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    events.clear();
  });

  it("shows actionable install guidance, installs automatically, and retries launch on success", async () => {
    const status: ProviderCliStatus = {
      required: true,
      installed: false,
      binary: "opaque-bin",
      resolved_path: null,
      availability: "unavailable",
      searched: ["/usr/bin"],
      installer: "npm install -g opaque-harness",
      updater: null,
      installer_prerequisites: [],
      updater_prerequisites: [],
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "provider_cli_status") return status;
      if (command === "install_provider_in_tab") return { tab_id: "install-1", launch_state: "command_dispatched" };
      return undefined;
    });

    const onRetry = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <TabLaunchFailure
            reason={MISSING_REASON}
            actions={[{ label: "Restart", onClick: onRetry }]}
          />,
        );
      });

      expect(host.textContent).toContain("Opaque Harness isn't installed");
      expect(host.textContent).toContain("npm install -g opaque-harness");
      const installBtn = host.querySelector<HTMLButtonElement>('[data-testid="provider-install-action"]')!;
      expect(installBtn.textContent).toBe("Install Opaque Harness");

      await act(async () => { installBtn.click(); });
      expect(host.textContent).toContain("Installing Opaque Harness");
      await act(async () => {
        events.get("provider-cli-completed")!({
          payload: {
            provider: "opaque-harness",
            operation: "install",
            tab_id: "install-1",
            success: true,
            summary: "installed",
          },
        });
      });

      expect(invoke).toHaveBeenCalledWith("provider_cli_status", { providerId: "opaque-harness", refresh: true });
      expect(onRetry).toHaveBeenCalledOnce();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("retries a remote tab only on its own peer's matching installer completion", async () => {
    const peerId = "fixture-peer";
    const providerId = "opaque-harness";
    vi.spyOn(webApi, "localJson").mockResolvedValue({
      ok: true, peer: { instance_id: peerId }, data: { features: ["provider_cli_completion_rail"] },
    });
    const status = { installer: "remote-installer", installer_prerequisites: [] };
    vi.mocked(invoke).mockImplementation(async command => command === "provider_cli_status"
      ? status : { tab_id: "installer", launch_state: "command_dispatched" });
    const retry = vi.fn();
    const host = document.createElement("div");
    const root = createRoot(host);
    const emit = (peer: string, id: string) => {
      for (const listener of peerObservers.get(peer) ?? []) listener({
        type: "provider-cli-completed",
        data: { provider: providerId, operation: "install", tab_id: id, success: true, summary: "installed" },
      });
    };
    try {
      await act(async () => root.render(
        <TabLaunchFailure tabId={`mesh:${peerId}/agent`} reason={MISSING_REASON}
          actions={[{ label: "Restart", onClick: retry }]} />,
      ));
      expect(invoke).toHaveBeenCalledWith("provider_cli_status", { providerId, peerId });
      expect(host.textContent).toContain("remote-installer");
      await act(async () => { host.querySelector<HTMLButtonElement>('[data-testid="provider-install-action"]')!.click(); });
      expect(invoke).toHaveBeenCalledWith("install_provider_in_tab", { providerId, peerId });
      expect(events.has("provider-cli-completed")).toBe(false);
      await act(async () => { emit("other-peer", "installer"); emit(peerId, "other-installer"); });
      expect(retry).not.toHaveBeenCalled();
      await act(async () => { emit(peerId, "installer"); });
      expect(invoke).toHaveBeenCalledWith("provider_cli_status", { providerId, peerId, refresh: true });
      expect(retry).toHaveBeenCalledOnce();
    } finally {
      await act(async () => root.unmount());
      peerObservers.clear();
    }
  });

  it("dispatches once on an older peer and retries only after that peer confirms installation", async () => {
    const peerId = "older-peer";
    const providerId = "opaque-harness";
    const identify = vi.spyOn(webApi, "localJson").mockResolvedValue({
      ok: true, peer: { instance_id: peerId }, data: { features: ["rest-api"] },
    });
    let installed: boolean | null = false;
    vi.mocked(invoke).mockImplementation(async command => command === "provider_cli_status"
      ? { installed, installer: "remote-installer", installer_prerequisites: [] }
      : { tab_id: "remote-installer-tab", launch_state: "command_dispatched" });
    const retry = vi.fn();
    const host = document.createElement("div");
    const root = createRoot(host);
    try {
      await act(async () => root.render(
        <TabLaunchFailure tabId={`mesh:${peerId}/agent`} reason={MISSING_REASON}
          actions={[{ label: "Restart", onClick: retry }]} />,
      ));
      await act(async () => { host.querySelector<HTMLButtonElement>('[data-testid="provider-install-action"]')!.click(); });
      expect(identify).toHaveBeenCalledExactlyOnceWith("POST", "/api/mesh/relay-to", {
        target_instance_id: peerId, method: "GET", path: "/api/identify", body: null,
      }, expect.objectContaining({ timeoutMs: expect.any(Number) }));
      expect(invoke).toHaveBeenCalledWith("install_provider_in_tab", { providerId, peerId });
      const manualRetry = host.querySelector<HTMLButtonElement>('[data-testid="provider-retry-action"]')!;
      expect(manualRetry.textContent).toBe("Retry once installed");
      expect(manualRetry.disabled).toBe(false);
      for (const result of [false, null, true]) {
        installed = result;
        await act(async () => { manualRetry.click(); });
        expect(invoke).toHaveBeenLastCalledWith("provider_cli_status", { providerId, peerId, refresh: true });
        expect(retry).toHaveBeenCalledTimes(result === true ? 1 : 0);
      }
      expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "install_provider_in_tab")).toHaveLength(1);
    } finally {
      await act(async () => root.unmount());
      peerObservers.clear();
    }
  });

  it("shows exact failure, official prerequisite install route, manual command, and retry when automatic install fails", async () => {
    openUrl.mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "provider_cli_status") {
        return {
          required: true,
          installed: false,
          binary: "opaque-bin",
          resolved_path: null,
          availability: "unavailable",
          searched: [],
          installer: "npm install -g opaque-harness",
          updater: null,
          installer_prerequisites: [{ binary: "npm", installed: false, hint: "Install npm using https://nodejs.org/en/download, then rerun the manifest command." }],
          updater_prerequisites: [],
        } satisfies ProviderCliStatus;
      }
      if (command === "install_provider_in_tab") return { tab_id: "install-2", launch_state: "command_dispatched" };
      return undefined;
    });

    const onRetry = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <TabLaunchFailure
            reason={MISSING_REASON}
            actions={[{ label: "Restart", onClick: onRetry }]}
          />,
        );
      });

      const installBtn = host.querySelector<HTMLButtonElement>('[data-testid="provider-install-action"]')!;
      await act(async () => { installBtn.click(); });
      await act(async () => {
        events.get("provider-cli-completed")!({
          payload: {
            provider: "opaque-harness",
            operation: "install",
            tab_id: "install-2",
            success: false,
            summary: "opaque-harness CLI install: command failed with exit code 127; npm not found",
          },
        });
      });

      expect(host.querySelector('[data-testid="provider-install-error"]')?.textContent).toContain("exit code 127; npm not found");
      expect(host.textContent).toContain("npm install -g opaque-harness");
      expect(host.textContent).toContain("Needs npm first.");
      const prereqBtn = host.querySelector<HTMLButtonElement>('[data-testid="prerequisite-install-link-npm"]')!;
      expect(prereqBtn).not.toBeNull();
      await act(async () => { prereqBtn.click(); });
      expect(openUrl).toHaveBeenCalledExactlyOnceWith("https://nodejs.org/en/download");
      expect(host.querySelector<HTMLButtonElement>('[data-testid="provider-install-action"]')?.textContent).toBe("Retry install");
      expect(onRetry).not.toHaveBeenCalled();

      const retryBtn = host.querySelector<HTMLButtonElement>('[data-testid="provider-retry-action"]')!;
      await act(async () => { retryBtn.click(); });
      expect(invoke).toHaveBeenCalledWith("provider_cli_status", { providerId: "opaque-harness", refresh: true });
      expect(onRetry).toHaveBeenCalledOnce();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
