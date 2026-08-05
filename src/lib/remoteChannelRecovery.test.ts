import { describe, expect, finish, it } from "../test/standaloneTest";
import { createRemoteChannelRecovery, createSilenceDeadline, parkStalledConnection } from "./remoteChannelRecovery";

const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));
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

  it("parkStalledConnection parks + aborts the current generation, no-ops otherwise", () => {
    const { recovery } = build();
    const gen = recovery.beginConnection();

    // Current generation -> park (awaiting signal) and tear down the socket.
    const silent = new AbortController();
    expect(parkStalledConnection(recovery, gen, silent)).toBe(true);
    expect(silent.signal.aborted).toBe(true);
    expect(recovery.isAwaitingSignal()).toBe(true);

    // Superseded generation -> no-op.
    const next = recovery.beginConnection();
    expect(next).toBe(gen + 1);
    const stale = new AbortController();
    expect(parkStalledConnection(recovery, gen, stale)).toBe(false);
    expect(stale.signal.aborted).toBe(false);
  });

  it("silence deadline parks after silence; re-arm resets it and clear cancels it", async () => {
    // Fires on genuine silence: no re-arm within the window -> park + abort.
    {
      const { recovery } = build();
      const gen = recovery.beginConnection();
      const ctl = new AbortController();
      const deadline = createSilenceDeadline(recovery, gen, ctl, 5);
      deadline.arm();
      await wait(40);
      expect(ctl.signal.aborted).toBe(true);
      expect(recovery.isAwaitingSignal()).toBe(true);
    }
    // Re-arm before expiry keeps it alive (a keep-alive on an idle-healthy tab);
    // clear() on teardown stops the chain so it cannot outlive the connection.
    {
      const { recovery } = build();
      const gen = recovery.beginConnection();
      const ctl = new AbortController();
      const deadline = createSilenceDeadline(recovery, gen, ctl, 30);
      deadline.arm();
      await wait(15); deadline.arm();
      await wait(15); deadline.arm();
      await wait(15);
      expect(ctl.signal.aborted).toBe(false); // never idle a full window straight.
      deadline.clear();
      await wait(50);
      expect(ctl.signal.aborted).toBe(false); // cleared: no park after teardown.
    }
  });
});

finish();
