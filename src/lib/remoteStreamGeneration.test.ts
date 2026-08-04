import { describe, expect, finish, it, vi } from "../test/standaloneTest";
import {
  remoteOutputSubscriptionGeneration, resumeRemoteOutputStreams, subscribeTabOutput,
  remoteOutputSubscriptionSnapshot,
  waitForRemoteOutputGenerationSettlement,
} from "./outputStream";

const encoder = new TextEncoder();
const frame = (body: Record<string, unknown>): Uint8Array =>
  encoder.encode(`data: ${JSON.stringify(body)}\n\n`);

// A stream that emits the given frames then stays open (no reconnect churn in-test).
const openStream = (...frames: Uint8Array[]): Response =>
  new Response(new ReadableStream({ start(controller) { for (const f of frames) controller.enqueue(f); } }));

// A stream that emits the given frames then closes -> clean EOF.
const closedStream = (...frames: Uint8Array[]): Response =>
  new Response(new ReadableStream({ start(controller) { for (const f of frames) controller.enqueue(f); controller.close(); } }));

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

  it("initial remote subscribe replays history then live output", async () => {
    vi.stubGlobal("window", new EventTarget());
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (...args: unknown[]) => {
      requests.push(String(args[0]));
      return openStream(
        frame({ boot: "boot-a", tab_id: "g1", output: "history", offset: 7 }),
        frame({ boot: "boot-a", tab_id: "g1", output: "live", offset: 11 }),
      );
    }));
    const outputs: string[] = [];
    let gotLive!: () => void;
    const liveDelivered = new Promise<void>((resolve) => { gotLive = resolve; });
    const unsubscribe = subscribeTabOutput("mesh:peer/g1", {
      onOutput(value) { outputs.push(value); if (value === "live") gotLive(); },
      onTruncated() {}, onEnded() {},
    });
    await liveDelivered;
    expect(requests[0]).not.toContain("since="); // fresh selection => full replay of initial history.
    expect(outputs).toEqual(["history", "live"]);
    unsubscribe();
    vi.unstubAllGlobals();
  });

  it("EOF reconnect resumes from cursor without duplicate", async () => {
    vi.stubGlobal("window", new EventTarget());
    const frameNew = frame({ boot: "boot-a", tab_id: "g2", output: "new", offset: 17 });
    const frameMore = frame({ boot: "boot-a", tab_id: "g2", output: "more", offset: 20 });
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (...args: unknown[]) => {
      requests.push(String(args[0]));
      // First stream delivers one frame then EOFs; the reconnect replays that same
      // frame (server resumes inclusively) plus one genuinely new frame.
      if (requests.length === 1) return closedStream(frameNew);
      return openStream(frameNew, frameMore);
    }));
    const outputs: string[] = [];
    let gotMore!: () => void;
    const moreDelivered = new Promise<void>((resolve) => { gotMore = resolve; });
    const unsubscribe = subscribeTabOutput("mesh:peer/g2", {
      onOutput(value) { outputs.push(value); if (value === "more") gotMore(); },
      onTruncated() {}, onEnded() {},
    });
    await moreDelivered;
    expect(requests.length).toBe(2);
    expect(requests[1]).toContain("since="); // reconnect carries the {boot,offset} cursor.
    expect(outputs).toEqual(["new", "more"]); // missed byte delivered once; replayed frame deduped.
    unsubscribe();
    vi.unstubAllGlobals();
  });
});

finish();
