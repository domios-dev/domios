import { expect, it } from "vitest";
import { parseNeighbors, validNeighborLimit } from "./neighborModel";
import { conciergeWork, readableBody, refusalLine } from "./neighborConversationModel";
import type { HistoryEntry } from "./neighborConciergeClient";

it("accepts the server's locked registry so Free can manage existing connections", () => {
  const list = { schema_version: 1, access: "locked", friends: [], incoming: [], outgoing: [] };
  expect(parseNeighbors(list)).toEqual(list);
  expect(() => parseNeighbors({ ...list, access: "free" })).toThrow();
  expect(() => parseNeighbors({ ...list, incoming: [null] })).toThrow();
});

it("preserves bounded own outgoing rows without inventing recipient profiles", () => {
  const outgoing = [{ id: "a".repeat(64), typed_email: "recipient@example.test", created_at: 42, state: "pending" }];
  const list = { schema_version: 1, access: "paid", friends: [], incoming: [], outgoing };
  expect(parseNeighbors(list).outgoing).toEqual(outgoing);
  for (const invalid of [undefined, [null], [{ ...outgoing[0], id: "bad" }], [{ ...outgoing[0], state: "accepted" }], Array(201).fill(outgoing[0])]) {
    expect(() => parseNeighbors({ ...list, outgoing: invalid })).toThrow();
  }
});

it("permits only integer inbound limits from one through two hundred", () => {
  for (const value of [1, 42, 200]) expect(validNeighborLimit(value)).toBe(true);
  for (const value of [0, -1, 201, 1.5, NaN]) expect(validNeighborLimit(value)).toBe(false);
});

it("preserves the server's receiving flag and refuses an untyped flag", () => {
  const friend = { uid: "peer", displayName: null, photoURL: null, friendship_id: "pair", since: 1, inbound_per_hour: 10, outbound_per_hour: 10, receiving: false };
  const list = { schema_version: 1, access: "paid", friends: [friend], incoming: [], outgoing: [] };
  expect(parseNeighbors(list).friends[0].receiving).toBe(false);
  expect(() => parseNeighbors({ ...list, friends: [{ ...friend, receiving: "false" }] })).toThrow();
});


it("drops the agent's attachment lines only for files that show as chips", () => {
  const shown = "a".repeat(32);
  const missing = "b".repeat(32);
  const body = `Here you go\n[Neighbor attachment: note.txt (77 bytes), handle ${shown}]\n[Neighbor attachment: data.bin (9 bytes), handle ${missing}]`;
  expect(readableBody(body, new Set([shown]))).toBe(`Here you go\n[Neighbor attachment: data.bin (9 bytes), handle ${missing}]`);
  expect(readableBody(body, new Set([shown, missing]))).toBe("Here you go");
  expect(readableBody(body, new Set())).toBe(body);
});

it("marks each friend message with your concierge's standing, and work only on the newest open one", () => {
  const ask = (message_id: string, extra: Partial<HistoryEntry> = {}): HistoryEntry => ({ direction: "in", kind: "question", author: "friend", body: "?", at: 1, message_id, ...extra });
  const entries: HistoryEntry[] = [
    ask("a"), ask("b", { outcome: "refused", reason: "No" }),
    ask("c"), { direction: "out", kind: "answer", author: { agent: "tab", name: "Concierge" }, body: "!", at: 2, message_id: "r", in_reply_to: "c" },
    ask("d"),
  ];
  const working = conciergeWork(entries, { working: true, missing: false });
  expect(working.get("b")).toEqual({ work: "refused" });
  expect(working.get("c")).toEqual({ work: "answered", reply: "r" });
  expect(working.get("d")).toEqual({ work: "working" });
  expect(working.has("a")).toBe(false);
  expect(conciergeWork(entries, { working: false, missing: true }).get("d")).toEqual({ work: "waiting" });
  expect(conciergeWork(entries, { working: false, missing: false }).has("d")).toBe(false);
});

it("explains each delivery refusal in plain words and says when to retry only when it is known", () => {
  const base: HistoryEntry = { direction: "out", kind: "question", author: { agent: "tab", name: "Concierge" }, body: "x", at: 0, message_id: "m", delivery_status: "not_delivered" };
  const now = 1_700_000_000_000;
  expect(refusalLine({ ...base, reason: "rate_limited", retry_after_seconds: 600 }, "Ada", now)).toBe("Not delivered: Ada is getting too many messages right now. Try again in 10m.");
  expect(refusalLine({ ...base, reason: "aggregate_limited" }, "Ada", now)).toBe("Not delivered: you have sent too many Neighbor messages for now.");
  expect(refusalLine({ ...base, reason: "recipient_paused" }, "Ada", now)).toContain("Ada");
  expect(refusalLine({ ...base, reason: "recipient_locked" }, "Ada", now)).toBe("Not delivered: Ada's plan pauses their Neighbor messages.");
  expect(refusalLine({ ...base, reason: "aggregate_daily_limited", limit: 200, resets_at: now + 9 * 3_600_000 }, "Ada", now))
    .toBe("You reached today's sending limit (200). It resets in 9h.");
  expect(refusalLine({ ...base, reason: "something_new" }, "Ada", now)).toBeNull();
});
