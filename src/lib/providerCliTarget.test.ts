import { describe, expect, it } from "vitest";
import { resolveProviderCliTarget } from "./providerCliTarget";

describe("resolveProviderCliTarget", () => {
  it("extracts the peer and remote tab id from a composite mesh tab", () => {
    expect(resolveProviderCliTarget({ callerTabId: "mesh:blackstar/tab-42" })).toEqual({
      peerId: "blackstar",
      callerTabId: "tab-42",
    });
    expect(resolveProviderCliTarget({ callerTabId: "mesh:blackstar/tab-42", peerId: "blackstar" })).toEqual({
      peerId: "blackstar",
      callerTabId: "tab-42",
    });
  });

  it("keeps ordinary tabs local unless an explicit peer is selected", () => {
    expect(resolveProviderCliTarget({ callerTabId: "tab-local" })).toEqual({
      peerId: undefined,
      callerTabId: "tab-local",
    });
    expect(resolveProviderCliTarget({ peerId: "blackstar" })).toEqual({
      peerId: "blackstar",
      callerTabId: undefined,
    });
    expect(resolveProviderCliTarget({ peerId: null })).toEqual({
      peerId: undefined,
      callerTabId: undefined,
    });
  });

  it("refuses invalid composite ids, blank peers, and mismatched peer targets", () => {
    expect(() => resolveProviderCliTarget({ callerTabId: "mesh:broken" })).toThrow("local fallback is forbidden");
    expect(() => resolveProviderCliTarget({ peerId: "   " })).toThrow("exact peer installation id");
    expect(() => resolveProviderCliTarget({ callerTabId: "mesh:peer-a/tab-1", peerId: "peer-b" }))
      .toThrow('does not match tab peer "peer-a"');
  });
});
