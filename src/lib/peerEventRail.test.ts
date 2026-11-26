import { expect, finish, it } from "../test/standaloneTest";
import { createPeerEventRail, type PeerEvent } from "./peerEventRail";
import type { TransportClient } from "./transportClient";

it("shares a link, resumes its cursor and reconciles once after each replay", () => {
  const calls: Parameters<TransportClient["subscribe"]>[] = [];
  const subscribe: TransportClient["subscribe"] = (...args) => { calls.push(args); return () => {}; };
  const rail = createPeerEventRail(() => ({ subscribe }) as TransportClient);
  const events: PeerEvent[] = [];
  const stop = rail.subscribe("peer", "key-one", event => events.push(event));
  const stopOther = rail.subscribe("peer", "key-one", () => {});
  expect(calls.length).toBe(1);
  const params = new URL(calls[0][0], "http://local").searchParams;
  expect(params.get("peer")).toBe("peer");
  expect(params.get("scope")).toBe("peer");
  expect(params.has("tab")).toBe(false);
  const receive = (kind: string, data: unknown, boot?: string, seq?: number) =>
    calls[calls.length - 1][1]("message", { event_type: kind, data, boot, seq });
  const status = { tab_id: "second", status: "Working" };
  receive("peer-replay-complete", null, "boot-a", 1);
  receive("tab-status-changed", status, "boot-a", 2);
  receive("tab-status-changed", status, "boot-a", 2);
  receive("appearance-committed", { event: "e1", machine: "peer", proposal: "p1", undo: false }, "boot-a", 3);
  receive("config-changed", {}, "boot-a", 4);
  receive("provider-cli-started", { provider: "claude" }, "boot-a", 5);
  receive("provider-cli-completed", { provider: "claude", operation: "install", tab_id: "installer", summary: "installed", success: true }, "boot-a", 6);
  receive("provider-cli-update-available", { provider: "codex" }, "boot-a", 7);
  expect(events).toEqual([
    { type: "reconcile" },
    { type: "tab-status-changed", data: status },
    { type: "appearance-committed", data: { event: "e1", machine: "peer", proposal: "p1", undo: false } },
    { type: "config-changed" },
    { type: "provider-cli-started", data: { provider: "claude" } },
    { type: "provider-cli-completed", data: { provider: "claude", operation: "install", tab_id: "installer", summary: "installed", success: true } },
    { type: "provider-cli-update-available", data: { provider: "codex" } },
  ]);
  calls[0][2]?.onEnd?.({ reason: "eof" });
  expect(calls.length).toBe(2);
  expect(new URL(calls[1][0], "http://local").searchParams.get("since")).toBe("boot-a:7");
  receive("overflow", { boot: "boot-b", next_seq: 4 });
  receive("mux-state-changed", {}, "boot-b", 4);
  receive("peer-replay-complete", null, "boot-b", 5);
  receive("peer-replay-complete", null, "boot-b", 5);
  expect(events.slice(7)).toEqual([{ type: "invalidate" }, { type: "reconcile" }]);
  calls[1][2]?.onEnd?.({ reason: "eof" });
  expect(new URL(calls[2][0], "http://local").searchParams.get("since")).toBe("boot-b:5");
  const stopNew = rail.subscribe("peer", "key-two", () => {});
  expect(calls[2][2]?.signal?.aborted).toBe(true);
  expect(new URL(calls[3][0], "http://local").searchParams.has("since")).toBe(false);
  stop();
  stopOther();
  expect(calls[3][2]?.signal?.aborted).toBe(false);
  stopNew();
});


it("invalidates an admitted rail closed before its replay boundary", () => {
  const calls: Parameters<TransportClient["subscribe"]>[] = [];
  const subscribe: TransportClient["subscribe"] = (...args) => { calls.push(args); return () => {}; };
  const rail = createPeerEventRail(() => ({ subscribe }) as TransportClient);
  const events: PeerEvent[] = [];
  const stop = rail.subscribe("peer", "key", event => events.push(event));
  calls[0][2]?.onReady?.();
  calls[0][2]?.onEnd?.({ reason: "eof" });
  expect(events).toEqual([{ type: "invalidate" }]);
  stop();
});

it("backs off exponentially with jitter on 401 refusal, refreshes credentials before retry, and blocks tight resume loops", async () => {
  let clock = 1_000;
  let nextHandle = 0;
  const timers = new Map<number, { dueAt: number; callback: () => void }>();
  const scheduler = {
    schedule(callback: () => void, timeoutMs: number) {
      const id = ++nextHandle;
      timers.set(id, { dueAt: clock + timeoutMs, callback });
      return id;
    },
    cancel(handle: unknown) {
      timers.delete(handle as number);
    },
  };
  const advance = async (ms: number) => {
    clock += ms;
    for (const [id, timer] of [...timers]) {
      if (timer.dueAt <= clock) {
        timers.delete(id);
        timer.callback();
      }
    }
    await Promise.resolve();
    await Promise.resolve();
  };

  const calls: Parameters<TransportClient["subscribe"]>[] = [];
  const subscribe: TransportClient["subscribe"] = (...args) => { calls.push(args); return () => {}; };
  let refreshes = 0;
  const rail = createPeerEventRail(() => ({ subscribe }) as TransportClient, {
    now: () => clock,
    random: () => 1,
    scheduler,
    firstMs: 1_000,
    capMs: 30_000,
    refreshCooldownMs: 5_000,
    refreshCredentials: async () => { refreshes += 1; },
  });

  const stop = rail.subscribe("ubunta", "link-1", () => {});
  expect(calls.length).toBe(1);

  calls[0][2]?.onEnd?.({ reason: "http", status: 401 });
  // Repeated resume() calls during backoff must never storm immediately.
  for (let i = 0; i < 10; i++) rail.resume("ubunta");
  expect(calls.length).toBe(1);

  await advance(999);
  expect(calls.length).toBe(1);
  await advance(1);
  expect(refreshes).toBe(1);
  expect(calls.length).toBe(2);

  // Second 401 doubles backoff to 2000ms and respects refresh cooldown.
  calls[1][2]?.onEnd?.({ reason: "http", status: 401 });
  rail.resume("ubunta");
  await advance(1_999);
  expect(calls.length).toBe(2);
  await advance(1);
  expect(refreshes).toBe(1);
  expect(calls.length).toBe(3);

  // Successful replay resets backoff back to 1000ms.
  calls[2][1]("message", { event_type: "peer-replay-complete", boot: "boot-a", seq: 1, data: null });
  calls[2][2]?.onEnd?.({ reason: "http", status: 401 });
  await advance(1_000);
  expect(calls.length).toBe(4);
  stop();
});

void finish();
