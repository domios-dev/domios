import { describe, expect, it, vi } from "vitest";
import {
  remoteTabEventSubscriptionSnapshot, resumeRemoteTabEvents, subscribeRemoteTabEvent,
  waitForRemoteTabEventGenerationSettlement,
} from "./remoteTabEvents";
import { onRemoteTransportState } from "./remoteTabTransportState";

describe("remote chat event subscription ownership", () => {
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
    const bytes = new TextEncoder().encode(
      'data: {"boot":"boot-a","seq":9,"event_type":"tab-chat-msg-tab","data":{"delta":"x"}}\n\n',
    );
    const requests: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (input: string | URL | Request) => {
      requests.push(String(input));
      return new Response(new ReadableStream({ start(controller) { controller.enqueue(bytes); } }));
    }));
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
});
