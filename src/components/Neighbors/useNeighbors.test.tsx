import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useNeighbors } from "./useNeighbors";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";

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
function Harness({ client, index }: { client: NeighborClient; index: number }) {
  states[index] = useNeighbors("owner", client);
  return null;
}
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  bridge.handlers.clear();
  vi.restoreAllMocks();
});

it("shares lists and referral claims, consumes pushed lists, and bounds focus reads", async () => {
  let now = 0;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  const list: NeighborList = { schema_version: 1, access: "locked", can_invite: false, friends: [], incoming: [], outgoing: [] };
  const client: NeighborClient = {
    list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })),
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<><Harness client={client} index={0} /><Harness client={client} index={1} /></>));
  expect(client.list).toHaveBeenCalledTimes(1);
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(1);
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(client.list).toHaveBeenCalledTimes(1);
  const pushed: NeighborList = { ...list, incoming: [{ uid: "peer", displayName: null, photoURL: null, request_id: "invite", created_at: 1 }] };
  await act(async () => {
    for (const handler of bridge.handlers.get("neighbors-changed") ?? []) handler({ payload: { uid: "owner", list: pushed } });
  });
  expect(states.map(state => state.list)).toEqual([pushed, pushed]);
  expect(client.list).toHaveBeenCalledTimes(1);
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(2);
  now = 59_999;
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(client.list).toHaveBeenCalledTimes(1);
  now = 60_000;
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(client.list).toHaveBeenCalledTimes(2);
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(2);
  await act(async () => states[0].refresh());
  expect(client.list).toHaveBeenCalledTimes(3);
  expect(states[1].list).toEqual(list);
});
