import { describe, expect, it, vi } from "vitest";
import {
  remoteTabEventSubscriptionSnapshot, resumeRemoteTabEvents, subscribeRemoteTabEvent,
} from "./remoteTabEvents";
import { onRemoteTransportState } from "./remoteTabTransportState";

describe("remote chat event subscription ownership", () => {
  it("atomically replaces generations and original unsubscribe owns the replacement", () => {
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    const event = "tab-chat-msg-mesh:peer/tab";
    const unsubscribe = subscribeRemoteTabEvent(event, () => undefined)!;
    expect(remoteTabEventSubscriptionSnapshot(event)?.generation).toBe(1);
    resumeRemoteTabEvents();
    expect(remoteTabEventSubscriptionSnapshot(event)?.generation).toBe(2);
    unsubscribe();
    expect(remoteTabEventSubscriptionSnapshot(event)).toBeNull();
    vi.unstubAllGlobals();
  });

  it("tracks validated cursor, publishes available, and replacement ownership is isolated", async () => {
    const bytes = new TextEncoder().encode(
      'data: {"boot":"boot-a","seq":9,"event_type":"tab-chat-msg-tab","data":{"delta":"x"}}\n\n',
    );
    vi.stubGlobal("fetch", vi.fn(async () => new Response(new ReadableStream({ start(controller) { controller.enqueue(bytes); } }))));
    const available: string[] = [];
    const stopState = onRemoteTransportState(({ availability }) => available.push(availability));
    let delivered!: () => void;
    const delivery = new Promise<void>((resolve) => { delivered = resolve; });
    const event = "tab-chat-msg-mesh:peer/tab";
    const first = subscribeRemoteTabEvent(event, () => delivered())!;
    await delivery;
    expect(remoteTabEventSubscriptionSnapshot(event)?.cursor).toBe("boot-a:9");
    expect(available).toContain("available");
    const second = subscribeRemoteTabEvent(event, () => undefined)!;
    first();
    expect(remoteTabEventSubscriptionSnapshot(event)).not.toBeNull();
    second();
    stopState();
    vi.unstubAllGlobals();
  });
});
