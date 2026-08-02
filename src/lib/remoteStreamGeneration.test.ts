import { describe, expect, it, vi } from "vitest";
import {
  remoteOutputSubscriptionGeneration, resumeRemoteOutputStreams, subscribeTabOutput,
  remoteOutputSubscriptionSnapshot,
  waitForRemoteOutputGenerationSettlement,
} from "./outputStream";

describe("remote output generation replacement", () => {
  it("old finalizer cannot clear the replacement generation", async () => {
    let settleOldFetch!: (response: Response) => void;
    const oldFetch = new Promise<Response>((resolve) => { settleOldFetch = resolve; });
    const fetchMock = vi.fn()
      .mockReturnValueOnce(oldFetch)
      .mockReturnValue(new Promise(() => {}));
    vi.stubGlobal("fetch", fetchMock);
    const id = "mesh:peer/tab";
    const unsubscribe = subscribeTabOutput(id, { onOutput() {}, onTruncated() {}, onEnded() {} });
    const first = remoteOutputSubscriptionGeneration(id);
    const oldSettled = waitForRemoteOutputGenerationSettlement(id, first);
    resumeRemoteOutputStreams();
    expect(remoteOutputSubscriptionGeneration(id)).toBe(first + 1);
    settleOldFetch(new Response(null));
    await oldSettled;
    expect(remoteOutputSubscriptionSnapshot(id)).toEqual({ generation: first + 1, hasController: true });
    unsubscribe();
    vi.unstubAllGlobals();
  });
});
