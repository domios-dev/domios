import { describe, expect, it, vi } from "vitest";
import {
  remoteOutputSubscriptionGeneration, resumeRemoteOutputStreams, subscribeTabOutput,
} from "./outputStream";

describe("remote output generation replacement", () => {
  it("authoritative resume replaces a dying controller immediately", () => {
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    const id = "mesh:peer/tab";
    const unsubscribe = subscribeTabOutput(id, { onOutput() {}, onTruncated() {}, onEnded() {} });
    const first = remoteOutputSubscriptionGeneration(id);
    resumeRemoteOutputStreams();
    expect(remoteOutputSubscriptionGeneration(id)).toBe(first + 1);
    unsubscribe();
    vi.unstubAllGlobals();
  });
});
