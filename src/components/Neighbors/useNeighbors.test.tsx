import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useNeighbors } from "./useNeighbors";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";
import { neighborReads } from "./neighborReads";

const bridge = vi.hoisted(() => ({ handlers: new Map<string, Set<(event: { payload: unknown }) => void>>() }));
vi.mock("../../lib/bridge", () => ({ listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
  const handlers = bridge.handlers.get(event) ?? new Set();
  bridge.handlers.set(event, handlers);
  handlers.add(handler);
  return () => { handlers.delete(handler); };
} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
const states: ReturnType<typeof useNeighbors>[] = [];
const list: NeighborList = { schema_version: 1, access: "paid", can_invite: false, friends: [], incoming: [], outgoing: [] };
const client = (): NeighborClient => ({ list: vi.fn(async () => list), apply: vi.fn(async () => {}), claimReferralTrial: vi.fn() });
function Harness({ port, index, identity = "owner" }: { port: NeighborClient; index: number; identity?: string }) {
  states[index] = useNeighbors(identity, port);
  return null;
}
function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  return root;
}
const emit = (event: string, payload: unknown = {}) => {
  for (const handler of bridge.handlers.get(event) ?? []) handler({ payload });
};
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  bridge.handlers.clear();
  vi.restoreAllMocks();
});

it("counts exactly one initial read across four consumers, ten focus events and five view opens, then one signal read", async () => {
  let now = 0;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  const port = client();
  const root = mount();
  await act(async () => root.render(<>{[0, 1, 2, 3].map(index => <Harness key={index} port={port} index={index} />)}</>));
  expect(port.list).toHaveBeenCalledTimes(1);
  for (let index = 0; index < 10; index++) {
    now += 360_000;
    await act(async () => window.dispatchEvent(new Event("focus")));
  }
  for (let index = 0; index < 5; index++) {
    await act(async () => root.render(null));
    await act(async () => root.render(<Harness port={port} index={0} />));
  }
  expect(port.list).toHaveBeenCalledTimes(1);
  expect(port.claimReferralTrial).toHaveBeenCalledTimes(0);
  await act(async () => emit("neighbors-changed", { uid: "owner", list: null }));
  expect(port.list).toHaveBeenCalledTimes(2);
  expect(port.claimReferralTrial).toHaveBeenCalledTimes(0);
});

it("consumes own-account snapshots even with no mounted view and ignores foreign pushes", async () => {
  const port = client();
  const root = mount();
  await act(async () => root.render(<Harness port={port} index={0} />));
  await act(async () => root.render(null));
  const pushed = { ...list, incoming: [{ uid: "peer", displayName: null, photoURL: null, request_id: "invite", created_at: 1 }] };
  await act(async () => emit("neighbors-changed", { uid: "owner", list: pushed }));
  await act(async () => emit("neighbors-changed", { uid: "other", list }));
  await act(async () => root.render(<Harness port={port} index={0} />));
  expect(states[0].list).toEqual(pushed);
  expect(port.list).toHaveBeenCalledTimes(1);
});

it("does not restore a late initial list over a pushed snapshot", async () => {
  let finish!: (value: NeighborList) => void;
  const port = client();
  vi.mocked(port.list).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const root = mount();
  await act(async () => root.render(<Harness port={port} index={0} />));
  const pushed = { ...list, access: "locked" as const };
  await act(async () => emit("neighbors-changed", { uid: "owner", list: pushed }));
  await act(async () => finish(list));
  expect(states[0].list).toEqual(pushed);
  expect(port.list).toHaveBeenCalledTimes(1);
});

it("coalesces invalidations across consumers while the initial read is pending", async () => {
  let finish!: (value: NeighborList) => void;
  const port = client();
  vi.mocked(port.list).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const root = mount();
  await act(async () => root.render(<><Harness port={port} index={0} /><Harness port={port} index={1} /></>));
  await act(async () => { emit("entitlement-changed"); emit("neighbors-changed", { uid: "owner", list: null }); });
  await act(async () => finish({ ...list, access: "locked" }));
  expect(port.list).toHaveBeenCalledTimes(2);
  expect(states[0].list).toEqual(list);
  expect(states[1].list).toEqual(list);
});

it("keeps account snapshots isolated and bounds sign-in refreshes without claiming on a view", async () => {
  const port = client();
  const first = neighborReads(port, "first")!;
  const root = mount();
  await act(async () => root.render(<Harness port={port} index={0} identity="first" />));
  await act(async () => emit("account-changed"));
  expect(port.list).toHaveBeenCalledTimes(2);
  await act(async () => root.render(<Harness port={port} index={0} identity="second" />));
  expect(neighborReads(port, "second")).not.toBe(first);
  expect(port.list).toHaveBeenCalledTimes(3);
  expect(port.claimReferralTrial).not.toHaveBeenCalled();
});

it("carries the server's persisted referral receipt in the shared snapshot", async () => {
  const port = client();
  const referral = { schema_version: 1 as const, outcome: "already_granted" as const, trial_end: 1700604800, friend_request_id: null };
  vi.mocked(port.list).mockResolvedValue({ ...list, referral });
  const root = mount();
  await act(async () => root.render(<Harness port={port} index={0} />));
  expect(states[0].referral).toEqual(referral);
  await act(async () => states[0].refresh());
  expect(port.list).toHaveBeenLastCalledWith(undefined, true);
  expect(port.claimReferralTrial).not.toHaveBeenCalled();
});
