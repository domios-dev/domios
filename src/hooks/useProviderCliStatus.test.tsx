import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PeerEvent } from "../lib/peerEventRail";
import type { ProviderCliStatus } from "../lib/providerCli";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(async () => () => {}),
  observers: new Map<string, Set<(event: PeerEvent) => void>>(),
}));

vi.mock("../lib/bridge", () => ({
  invoke: mocks.invoke,
  listen: mocks.listen,
}));

vi.mock("../lib/peerEventRail", () => ({
  peerEventRail: {
    observe: (peerId: string, listener: (event: PeerEvent) => void) => {
      const set = mocks.observers.get(peerId) ?? new Set();
      set.add(listener);
      mocks.observers.set(peerId, set);
      return () => {
        set.delete(listener);
        if (!set.size) mocks.observers.delete(peerId);
      };
    },
  },
}));

import { useProviderCliStatus } from "./useProviderCliStatus";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const baseStatus: ProviderCliStatus = {
  required: true,
  installed: true,
  binary: "claude",
  resolved_path: "/tools/claude",
  availability: "available",
  searched: [],
  installer: null,
  updater: "claude update",
  latest_version: "2.0.0",
};

beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.listen.mockClear();
  mocks.observers.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe("useProviderCliStatus", () => {
  it("queries the remote peer and reacts to peer-rail provider-cli-* events", async () => {
    let latest: ReturnType<typeof useProviderCliStatus> | undefined;
    function Probe({ peerId }: { peerId?: string | null }) {
      latest = useProviderCliStatus("claude", peerId);
      return null;
    }

    mocks.invoke.mockResolvedValueOnce(baseStatus);
    await act(async () => root.render(<Probe peerId="blackstar" />));
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("provider_cli_status", {
      providerId: "claude",
      peerId: "blackstar",
    });
    expect(mocks.listen).not.toHaveBeenCalled();
    expect(latest?.status?.latest_version).toBe("2.0.0");

    // Events for another provider on the same peer are ignored.
    await act(async () => {
      for (const listener of mocks.observers.get("blackstar") ?? []) {
        listener({ type: "provider-cli-completed", data: { provider: "codex", summary: "codex updated" } });
      }
    });
    expect(mocks.invoke).toHaveBeenCalledTimes(1);

    // Matching provider-cli-completed updates summary and refreshes the remote status.
    mocks.invoke.mockResolvedValueOnce({
      ...baseStatus,
      latest_version: null,
      update_job: { tab_id: "cli-1", state: "succeeded", summary: "claude CLI updated to 2.0.0" },
    });
    await act(async () => {
      for (const listener of mocks.observers.get("blackstar") ?? []) {
        listener({
          type: "provider-cli-completed",
          data: { provider: "claude", summary: "claude CLI updated to 2.0.0", success: true },
        });
      }
    });
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    expect(latest?.summary).toBe("claude CLI updated to 2.0.0");
    expect(latest?.status?.update_job?.state).toBe("succeeded");

    // Switching peer resets old state and queries the new machine.
    mocks.invoke.mockResolvedValueOnce({ ...baseStatus, latest_version: "3.0.0" });
    await act(async () => root.render(<Probe peerId="mac-mini" />));
    expect(mocks.observers.has("blackstar")).toBe(false);
    expect(mocks.invoke).toHaveBeenLastCalledWith("provider_cli_status", {
      providerId: "claude",
      peerId: "mac-mini",
    });
    expect(latest?.status?.latest_version).toBe("3.0.0");
    expect(latest?.summary).toBeNull();
  });
});
