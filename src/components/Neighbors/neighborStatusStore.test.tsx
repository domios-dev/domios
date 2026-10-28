import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { ChannelStatus, NeighborListenerEvent, NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useReceiverState } from "./useReceiverState";

vi.mock("../../lib/bridge", () => ({ listen: vi.fn(async () => () => {}) }));

const status = (receiving_here: boolean, revision: number): ChannelStatus =>
  ({ enrolled: true, bindings: {}, receiving_here, receiving_enabled: true, receiver_machine_label: "desk", revision });

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
  const Probe = ({ on }: { on: boolean }) => { seen.push(useReceiverState(runtime, on)?.receiving_here ?? null); return null; };
  unmount = () => act(() => root.unmount());
  return { seen, render: (on: boolean) => act(async () => root.render(<Probe on={on} />)), first: act(async () => root.render(<Probe on={active} />)) };
}

it("never lets a late first answer replace a newer receiver event", async () => {
  const old = deferred<ChannelStatus>();
  const { runtime, emit } = client(old.promise, Promise.resolve(status(true, 2)));
  const probe = show(runtime, true);
  await probe.first;
  await act(async () => emit(status(true, 2)));
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
  await act(async () => emit(status(true, 5)));
  expect(probe.seen[probe.seen.length - 1]).toBe(true);
});
