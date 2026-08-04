import { afterEach, describe, expect, finish, it, vi } from "../test/standaloneTest";
import {
  remoteTabEventSubscriptionSnapshot, resumeRemoteTabEvents, subscribeRemoteTabEvent,
  waitForRemoteTabEventGenerationSettlement,
} from "./remoteTabEvents";
import { onRemoteTransportState } from "./remoteTabTransportState";

describe("remote chat event subscription ownership", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("old finalizer cannot clear replacement and original unsubscribe owns it", async () => {
    let settleOldFetch!: (response: Response) => void;
    const oldFetch = new Promise<Response>((resolve) => { settleOldFetch = resolve; });
    vi.stubGlobal("fetch", vi.fn().mockReturnValueOnce(oldFetch).mockReturnValue(new Promise(() => {})));
    const event = "tab-chat-msg-mesh:peer/tab";
    const unsubscribe = subscribeRemoteTabEvent(event, () => undefined)!;
    expect(remoteTabEventSubscriptionSnapshot(event)?.generation).toBe(1);
    const oldSettled = waitForRemoteTabEventGenerationSettlement(event, 1);
    resumeRemoteTabEvents();
    expect(remoteTabEventSubscriptionSnapshot(event)?.generation).toBe(2);
    settleOldFetch(new Response(null));
    await oldSettled;
    expect(remoteTabEventSubscriptionSnapshot(event)).toMatchObject({ generation: 2, hasController: true });
    unsubscribe();
    expect(remoteTabEventSubscriptionSnapshot(event)).toBeNull();
    vi.unstubAllGlobals();
  });

  it("tracks validated cursor, publishes available, and replacement ownership is isolated", async () => {
    vi.stubGlobal("window", new EventTarget());
    const bytes = new TextEncoder().encode(
      'data: {"boot":"boot-a","seq":9,"event_type":"tab-chat-msg-tab","data":{"delta":"x"}}\n\n',
    );
    const requests: string[] = [];
    const fetchMock: typeof fetch = async (input) => {
      requests.push(String(input));
      return new Response(new ReadableStream({ start(controller) { controller.enqueue(bytes); } }));
    };
    vi.stubGlobal("fetch", fetchMock);
    const available: string[] = [];
    const stopState = onRemoteTransportState(({ availability }) => available.push(availability));
    let delivered!: () => void;
    const delivery = new Promise<void>((resolve) => { delivered = resolve; });
    const event = "tab-chat-msg-mesh:peer/tab";
    const first = subscribeRemoteTabEvent(event, () => delivered())!;
    await delivery;
    expect(remoteTabEventSubscriptionSnapshot(event)?.cursor).toBe("boot-a:9");
    expect(available).toContain("available");
    resumeRemoteTabEvents();
    expect(requests[1]).toContain("since=boot-a%3A9");
    const second = subscribeRemoteTabEvent(event, () => undefined)!;
    first();
    expect(remoteTabEventSubscriptionSnapshot(event)).not.toBeNull();
    second();
    stopState();
    vi.unstubAllGlobals();
  });

  it("authoritative snapshot then live events delivered", async () => {
    vi.stubGlobal("window", new EventTarget());
    // Frame 1 = authoritative snapshot; 2/4 = live deltas; replayed seq-2 dedups (G3).
    const frames = [
      '{"boot":"b","seq":1,"event_type":"tab-chat-msg-tab","data":{"delta":"snapshot"}}',
      '{"boot":"b","seq":2,"event_type":"tab-chat-msg-tab","data":{"delta":"live-1"}}',
      '{"boot":"b","seq":2,"event_type":"tab-chat-msg-tab","data":{"delta":"dup"}}',
      '{"boot":"b","seq":3,"event_type":"tab-chat-msg-tab","data":{"delta":"live-2"}}',
    ];
    const bytes = new TextEncoder().encode(frames.map((f) => `data: ${f}\n\n`).join(""));
    const fetchMock: typeof fetch = async () => new Response(new ReadableStream({
      start(controller) { controller.enqueue(bytes); controller.close(); },
    }));
    vi.stubGlobal("fetch", fetchMock);
    const available: string[] = [];
    const stopState = onRemoteTransportState(({ channel, availability }) => {
      if (channel === "chat") available.push(availability);
    });
    const deltas: string[] = [];
    let resolveThird!: () => void;
    const gotThird = new Promise<void>((resolve) => { resolveThird = resolve; });
    const event = "tab-chat-msg-mesh:peer/tab";
    const unsubscribe = subscribeRemoteTabEvent<{ delta: string }>(event, ({ payload }) => {
      deltas.push(payload.delta);
      if (deltas.length === 3) resolveThird();
    })!;
    await gotThird;
    expect(deltas).toEqual(["snapshot", "live-1", "live-2"]);
    expect(available[0]).toBe("available");
    expect(remoteTabEventSubscriptionSnapshot(event)?.cursor).toBe("b:3");
    unsubscribe();
    stopState();
    vi.unstubAllGlobals();
  });
});

finish();
