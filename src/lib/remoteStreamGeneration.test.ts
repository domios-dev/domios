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
  it("resume does not supersede a connection awaiting its first frame", async () => {
    // In-flight fetch, no frame delivered yet: no cursor, not parked. Mount /
    // mesh-state-changed / credential churn must NOT restart it mid initial replay,
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(new Promise(() => {})));
    const id = "mesh:peer/tab";
    const unsubscribe = subscribeTabOutput(id, { onOutput() {}, onTruncated() {}, onEnded() {} });
    const first = remoteOutputSubscriptionGeneration(id);
    resumeRemoteOutputStreams();
    expect(remoteOutputSubscriptionGeneration(id)).toBe(first);
    expect(remoteOutputSubscriptionSnapshot(id)).toEqual({ generation: first, hasController: true });
    unsubscribe();
    vi.unstubAllGlobals();
  });

  it("a superseded generation's finalizer cannot clear the replacement's controller", async () => {
    vi.stubGlobal("window", new EventTarget());
    // gen1 delivers a frame (cursor set) then stays open; gen2 stays pending.
    const fetchMock = vi.fn()
      .mockReturnValueOnce(Promise.resolve(openStream(frame({ output: "hi", boot: "boot-a", offset: 1 }))))
      .mockReturnValue(new Promise(() => {}));
    vi.stubGlobal("fetch", fetchMock);
    const id = "mesh:peer/tab";
    let delivered!: () => void;
    const gotFrame = new Promise<void>((resolve) => { delivered = resolve; });
    const unsubscribe = subscribeTabOutput(id, { onOutput() { delivered(); }, onTruncated() {}, onEnded() {} });
    await gotFrame;
    const first = remoteOutputSubscriptionGeneration(id);
    const oldSettled = waitForRemoteOutputGenerationSettlement(id, first);
    resumeRemoteOutputStreams(); // cursor is set now, so this legitimately supersedes
    expect(remoteOutputSubscriptionGeneration(id)).toBe(first + 1);
    await oldSettled; // the old connection finalizes after its abort
    // gen1's stale finalizer (decision === "stale") must not clear gen2's controller.
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
