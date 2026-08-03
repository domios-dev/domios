import { describe, expect, finish, it } from "../test/standaloneTest";
import { createRemoteChannelRecovery } from "./remoteChannelRecovery";
import type { RemoteTransportAvailability, RemoteTransportChannel } from "./remoteTabTransportState";

const advanced = (prev: number | null, next: number): boolean => prev === null || next > prev;

function build() {
  const emitted: Array<{ channel: RemoteTransportChannel; availability: RemoteTransportAvailability }> = [];
  const recovery = createRemoteChannelRecovery<number>({
    channel: "output",
    advanced,
    onAvailability: (channel, availability) => emitted.push({ channel, availability }),
  });
  return { recovery, emitted };
}

describe("remote channel recovery", () => {
  it("stale generation isolation", () => {
    const { recovery, emitted } = build();
    const first = recovery.beginConnection();
    expect(recovery.noteProgress(first, 10)).toBe(true);
    expect(recovery.currentCursor()).toBe(10);
    expect(recovery.availability()).toBe("available");

    const second = recovery.beginConnection();
    expect(second).toBe(first + 1);

    // A callback tagged with the superseded generation must not mutate cursor or state.
    expect(recovery.noteProgress(first, 999)).toBe(false);
    expect(recovery.currentCursor()).toBe(10);
    expect(recovery.handleStreamEnd(first, "error")).toBe("stale");
    expect(recovery.availability()).toBe("available");
    // Only the current generation drives real progress.
    expect(recovery.noteProgress(second, 20)).toBe(true);
    expect(recovery.currentCursor()).toBe(20);
    // No transition ever left 'available' here, so neither the live nor the stale
    // callbacks emitted anything — stale generations never touch availability.
    expect(emitted).toEqual([]);
  });

  it("zero-progress clean EOF does not busy-reconnect", () => {
    const { recovery, emitted } = build();
    const gen = recovery.beginConnection();
    // No frame arrived: a clean EOF here must NOT auto-reconnect (no busy-loop).
    expect(recovery.handleStreamEnd(gen, "eof")).toBe("wait");
    expect(recovery.isAwaitingSignal()).toBe(true);
    expect(recovery.availability()).toBe("reconnecting");
    expect(emitted).toEqual([{ channel: "output", availability: "reconnecting" }]);
    // Only an explicit external signal resumes the channel.
    expect(recovery.signal()).toBe("reconnect");
    expect(recovery.isAwaitingSignal()).toBe(false);
  });

  it("clean EOF with progress re-arms on every stream end", () => {
    const { recovery } = build();
    const g1 = recovery.beginConnection();
    recovery.noteProgress(g1, 5);
    expect(recovery.handleStreamEnd(g1, "eof")).toBe("reconnect");
    // Re-arm: a second progress-making stream end reconnects again (never one-shot).
    const g2 = recovery.beginConnection();
    recovery.noteProgress(g2, 12);
    expect(recovery.handleStreamEnd(g2, "eof")).toBe("reconnect");
    expect(recovery.currentCursor()).toBe(12);
  });

  it("connection error waits for a signal even after progress", () => {
    const { recovery } = build();
    const gen = recovery.beginConnection();
    recovery.noteProgress(gen, 7);
    expect(recovery.handleStreamEnd(gen, "error")).toBe("wait");
    expect(recovery.isAwaitingSignal()).toBe(true);
    expect(recovery.availability()).toBe("reconnecting");
  });

  it("suppresses duplicate and regressed cursors", () => {
    const { recovery } = build();
    const gen = recovery.beginConnection();
    expect(recovery.noteProgress(gen, 30)).toBe(true);
    expect(recovery.noteProgress(gen, 30)).toBe(false);
    expect(recovery.noteProgress(gen, 20)).toBe(false);
    expect(recovery.currentCursor()).toBe(30);
  });

  it("drops all callbacks after dispose", () => {
    const { recovery } = build();
    const gen = recovery.beginConnection();
    recovery.dispose();
    expect(recovery.isCurrent(gen)).toBe(false);
    expect(recovery.noteProgress(gen, 100)).toBe(false);
    expect(recovery.handleStreamEnd(gen, "eof")).toBe("stale");
    expect(recovery.signal()).toBe("stale");
  });
});

finish();
