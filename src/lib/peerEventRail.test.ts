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
  expect(events).toEqual([{ type: "reconcile" }, { type: "tab-status-changed", data: status }]);
  calls[0][2]?.onEnd?.({ reason: "eof" });
  expect(calls.length).toBe(2);
  expect(new URL(calls[1][0], "http://local").searchParams.get("since")).toBe("boot-a:2");
  receive("overflow", { boot: "boot-b", next_seq: 4 });
  receive("mux-state-changed", {}, "boot-b", 4);
  receive("peer-replay-complete", null, "boot-b", 5);
  receive("peer-replay-complete", null, "boot-b", 5);
  expect(events.slice(2)).toEqual([{ type: "reconcile" }]);
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

void finish();
