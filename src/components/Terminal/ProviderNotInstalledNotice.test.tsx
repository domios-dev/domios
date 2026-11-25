import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderCliStatus } from "../../lib/providerCli";

const events = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    events.set(name, handler);
    return () => { events.delete(name); };
  }),
}));

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

  it("shows exact failure, manual command, and retry when automatic install fails", async () => {
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
          installer_prerequisites: [{ binary: "npm", installed: false, hint: "Install Node.js first." }],
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
