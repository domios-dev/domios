import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { ChannelStatus, NeighborListenerEvent, NeighborRuntimeClient, ReceiverState } from "./neighborRuntimeClient";
import { friendReceiverState, useReceiverState } from "./useReceiverState";

vi.mock("../../lib/bridge", () => ({ listen: vi.fn(async () => () => {}) }));

const receiver = (receiving_here: boolean, revision: number): ReceiverState =>
  ({ receiving_here, receiving_enabled: true, receiver_machine_label: "desk", revision });
const status = (here: boolean, revision: number): ChannelStatus =>
  ({ enrolled: true, bindings: {}, friend_receivers_version: revision, friend_receivers: { first: receiver(here, revision) } });

function deferred<T>() {
  let resolve!: (value: T) => void;
  return { promise: new Promise<T>(done => { resolve = done; }), resolve };
}

function client(...replies: Array<Promise<ChannelStatus>>) {
  let emit: (event: NeighborListenerEvent) => void = () => {};
  const runtime = {
    status: vi.fn(() => replies.shift() ?? new Promise<ChannelStatus>(() => {})),
    subscribe: vi.fn(async (handler: (event: NeighborListenerEvent) => void) => { emit = handler; return () => {}; }),
  } as unknown as NeighborRuntimeClient;
  return { runtime, emit: (event: NeighborListenerEvent) => emit(event) };
}

let unmount: (() => void) | undefined;
afterEach(() => { unmount?.(); unmount = undefined; });

function show(runtime: NeighborRuntimeClient, active: boolean) {
  const seen: Array<boolean | null> = [];
  const host = document.createElement("div");
  const root = createRoot(host);
  const Probe = ({ on }: { on: boolean }) => { seen.push(useReceiverState(runtime, on, "first")?.receiving_here ?? null); return null; };
  unmount = () => act(() => root.unmount());
  return { seen, render: (on: boolean) => act(async () => root.render(<Probe on={on} />)), first: act(async () => root.render(<Probe on={active} />)) };
}

it("never lets a late first answer replace a newer receiver event", async () => {
  const old = deferred<ChannelStatus>();
  const { runtime, emit } = client(old.promise, Promise.resolve(status(true, 2)));
  const probe = show(runtime, true);
  await probe.first;
  await act(async () => emit({ ...receiver(true, 2), friendship_id: "first", friend_receivers_version: 2 }));
  await act(async () => old.resolve(status(false, 1)));
  expect(probe.seen[probe.seen.length - 1]).toBe(true);
  expect(probe.seen).not.toContain(false);
});

it("stays empty when an answer lands after the surface turned off", async () => {
  const late = deferred<ChannelStatus>();
  const { runtime } = client(late.promise);
  const probe = show(runtime, true);
  await probe.first;
  await probe.render(false);
  await act(async () => late.resolve(status(true, 1)));
  expect(probe.seen[probe.seen.length - 1]).toBeNull();
});

it("keeps a newer receiver event when the read it triggers answers with an older revision", async () => {
  const { runtime, emit } = client(Promise.resolve(status(false, 4)), Promise.resolve(status(false, 4)));
  const probe = show(runtime, true);
  await probe.first;
  await act(async () => emit({ ...receiver(true, 5), friendship_id: "first", friend_receivers_version: 5 }));
  expect(probe.seen[probe.seen.length - 1]).toBe(true);
});

it("friend lookup validates its own entry and never falls back to the account", () => {
  const initial = { ...status(true, 1), friend_receivers: { first: { ...receiver(false, 9) } } };
  expect(friendReceiverState(initial, "first")?.receiving_here).toBe(false);
  expect(friendReceiverState(initial, "missing")).toBeNull();
  expect(friendReceiverState({ ...initial, friend_receivers: { first: { ...receiver(true, 1), revision: -1 } } }, "first")).toBeNull();
});

it("a friend reset changes only its own authority", async () => {
  const initial = { ...status(true, 3), friend_receivers: { first: { ...receiver(true, 9) }, second: receiver(true, 3) } };
  const { runtime, emit } = client(Promise.resolve(initial));
  const host = document.createElement("div");
  const root = createRoot(host);
  let seen: Array<boolean | null> = [];
  const Probe = () => {
    seen = [useReceiverState(runtime, true, "first")?.receiving_here ?? null,
      useReceiverState(runtime, true, "second")?.receiving_here ?? null];
    return null;
  };
  unmount = () => act(() => root.unmount());
  await act(async () => root.render(<Probe />));
  expect(seen).toEqual([true, true]);
  await act(async () => emit({ ...receiver(false, 10), friendship_id: "first" }));
  expect(seen).toEqual([false, true]);
  await act(async () => emit({ receiver_unavailable: true, friendship_id: "first" }));
  expect(seen).toEqual([null, true]);
});

it.each([
  ["friend", 10],
  ["friend", 11],
] as const)("keeps a confirmed %s friend event when its reread returns the prior snapshot", async (_source, revision) => {
  const stale = deferred<ChannelStatus>();
  const initial: ChannelStatus = { ...status(true, 3), friend_receivers_version: 1,
    friend_receivers: { first: { ...receiver(false, 9) } } };
  const { runtime, emit } = client(Promise.resolve(initial), stale.promise);
  const host = document.createElement("div");
  const root = createRoot(host);
  let seen: ReturnType<typeof useReceiverState> = null;
  const Probe = () => { seen = useReceiverState(runtime, true, "first"); return null; };
  unmount = () => act(() => root.unmount());
  await act(async () => root.render(<Probe />));
  await act(async () => emit({ ...receiver(true, revision), friendship_id: "first", friend_receivers_version: 2 }));
  await act(async () => stale.resolve(initial));
  expect(seen).toMatchObject({ receiving_here: true, revision });
});

it("ignores deferred friend publication older than a newer invalidation", async () => {
  const initial: ChannelStatus = { ...status(true, 3), friend_receivers_version: 1,
    friend_receivers: { first: { ...receiver(true, 9) } } };
  const { runtime, emit } = client(Promise.resolve(initial));
  const host = document.createElement("div");
  const root = createRoot(host);
  let seen: ReturnType<typeof useReceiverState> = null;
  const Probe = () => { seen = useReceiverState(runtime, true, "first"); return null; };
  unmount = () => act(() => root.unmount());
  await act(async () => root.render(<Probe />));
  await act(async () => emit({ receiver_unavailable: true, friend_receivers_version: 2 }));
  expect(seen).toBeNull();
  await act(async () => emit({ ...receiver(true, 9), friendship_id: "first", friend_receivers_version: 1 }));
  expect(seen).toBeNull();
  expect(runtime.status).toHaveBeenCalledTimes(1);
});

it("shares receiver status on open across concurrent consumers", async () => {
  const pending = deferred<ChannelStatus>();
  const { runtime } = client(pending.promise);
  const host = document.createElement("div");
  const root = createRoot(host);
  let seen: Array<boolean | null> = [];
  const Probe = () => {
    seen = [useReceiverState(runtime, true, "first")?.receiving_here ?? null,
      useReceiverState(runtime, true, "first")?.receiving_here ?? null,
      useReceiverState(runtime, true, "first")?.receiving_here ?? null];
    return null;
  };
  unmount = () => act(() => root.unmount());
  await act(async () => root.render(<Probe />));
  expect(runtime.status).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve(status(true, 1)));
  expect(runtime.status).toHaveBeenCalledTimes(1);
  expect(seen).toEqual([true, true, true]);
});

it.each([4, 5])("keeps an unversioned friend event at revision %s when its reread returns the old receiver", async revision => {
  const initial = { enrolled: true, bindings: {}, friend_receivers: {
    first: { ...receiver(false, 4), receiver_machine_label: "Studio" },
  } };
  const { runtime, emit } = client(Promise.resolve(initial), Promise.resolve(initial));
  const host = document.createElement("div");
  const root = createRoot(host);
  let seen: ReturnType<typeof useReceiverState> = null;
  const Probe = () => { seen = useReceiverState(runtime, true, "first"); return null; };
  unmount = () => act(() => root.unmount());
  await act(async () => root.render(<Probe />));
  await act(async () => emit({ ...receiver(false, revision), receiver_machine_label: "Laptop", friendship_id: "first" }));
  expect(seen).toMatchObject({ revision, receiver_machine_label: "Laptop" });
});
