import { describe, expect, it, vi } from "vitest";
import { onRemoteTransportState, publishRemoteTransportState } from "./remoteTabTransportState";

describe("remote transport availability", () => {
  it("projects stream failure and event-driven recovery without changing identity", () => {
    const seen = vi.fn();
    const stop = onRemoteTransportState(seen);
    publishRemoteTransportState("mesh:peer/tab", "unavailable");
    publishRemoteTransportState("mesh:peer/tab", "reconnecting");
    expect(seen.mock.calls.map(([value]) => value)).toEqual([
      { ordinaryTabId: "mesh:peer/tab", availability: "unavailable" },
      { ordinaryTabId: "mesh:peer/tab", availability: "reconnecting" },
    ]);
    stop();
  });
});
