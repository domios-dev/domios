import { describe, expect, finish, it, vi } from "../test/standaloneTest";
import {
  remoteOutputSubscriptionGeneration, resumeRemoteOutputStreams, subscribeTabOutput,
  remoteOutputSubscriptionSnapshot,
  waitForRemoteOutputGenerationSettlement,
} from "./outputStream";
import { onRemoteTransportState } from "./remoteTabTransportState";

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

  it("resumes once after EOF with the exact remote cursor", async () => {
    vi.stubGlobal("window", new EventTarget());
    const frame = new TextEncoder().encode(
      'data: {"boot":"boot-a","tab_id":"tab","output":"new","offset":17}\n\n',
    );
    const requests: string[] = [];
    let holdSecond!: () => void;
    const second = new Promise<Response>((resolve) => { holdSecond = () => resolve(new Response(null)); });
    vi.stubGlobal("fetch", vi.fn(async (...args: unknown[]) => {
      requests.push(String(args[0]));
      if (requests.length === 1) {
        return new Response(new ReadableStream({ start(controller) { controller.enqueue(frame); controller.close(); } }));
      }
      return second;
    }));
    const states: string[] = [];
    let reconnecting!: () => void;
    const reconnected = new Promise<void>((resolve) => { reconnecting = resolve; });
    const stopState = onRemoteTransportState(({ channel, availability }) => {
      states.push(`${channel}:${availability}`);
      if (channel === "output" && availability === "reconnecting") reconnecting();
    });
    let delivered!: () => void;
    const delivery = new Promise<void>((resolve) => { delivered = resolve; });
    const unsubscribe = subscribeTabOutput("mesh:peer/tab", {
      onOutput(value) { expect(value).toBe("new"); delivered(); },
      onTruncated() {},
      onEnded() {},
    });
    await delivery;
    await waitForRemoteOutputGenerationSettlement("mesh:peer/tab", 1);
    await reconnected;
    expect(requests.length).toBe(2);
    expect(requests[1]).toContain("since=");
    expect(states).toContain("output:reconnecting");
    unsubscribe();
    holdSecond();
    stopState();
    vi.unstubAllGlobals();
  });
});

finish();
