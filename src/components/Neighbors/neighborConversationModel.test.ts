import { expect, it } from "vitest";
import type { HistoryEntry } from "./neighborConciergeClient";
import { conversationMessages, mergeHistory, type OutgoingSend } from "./neighborConversationModel";

const messageId = "a".repeat(32);
const send: OutgoingSend = { key: "bubble", messageId, body: "Opaque message", at: 100, state: "sending" };
const confirmed: HistoryEntry = { direction: "out", author: "me", kind: "question", message_id: messageId,
  body: send.body, at: 101, delivery_status: "delivered", submission: "accepted" };

const orders = [
  ["response", "event", "sync"], ["response", "sync", "event"],
  ["event", "response", "sync"], ["event", "sync", "response"],
  ["sync", "response", "event"], ["sync", "event", "response"],
];

it.each(orders)("keeps exactly one message through %s, %s, %s and duplicate events", (...order) => {
  let entries: HistoryEntry[] = [];
  let outgoing = [send];
  expect(conversationMessages(outgoing, entries).shown).toHaveLength(1);
  for (const arrival of [...order, "event", "event", "sync"]) {
    if (arrival !== "response") entries = mergeHistory(entries, [confirmed]);
    const view = conversationMessages(outgoing, entries);
    expect(view.shown).toHaveLength(1);
    expect(view.shown[0].body).toBe("Opaque message");
    if (arrival !== "response" || entries.length > 0) expect(view.shown[0].message_id).toBe(messageId);
    outgoing = outgoing.filter(item => !view.landed.has(item.key));
    expect(conversationMessages(outgoing, entries).shown).toHaveLength(1);
  }
  expect(entries).toEqual([confirmed]);
  expect(outgoing).toEqual([]);
});

it("keeps identical sends separate and never matches another message by its words or clock", () => {
  const other = { ...send, key: "second", messageId: "b".repeat(32) };
  const unrelated = { ...confirmed, message_id: "c".repeat(32), at: 100 };
  let entries = mergeHistory([], [unrelated, { ...confirmed, message_id: other.messageId }]);
  expect(conversationMessages([send, other], entries).shown).toHaveLength(3);
  expect([...conversationMessages([send, other], entries).landed]).toEqual([["second", other.messageId]]);
  entries = mergeHistory(entries, [confirmed]);
  const view = conversationMessages([send, other], entries);
  expect(view.shown).toHaveLength(3);
  expect([...view.landed]).toEqual([["bubble", messageId], ["second", other.messageId]]);
});

it("retains a failed bubble for retry and ignores a late confirmation of its refused attempt", () => {
  const failed: OutgoingSend = { ...send, state: "failed", reason: "Refused before sending" };
  expect(conversationMessages([failed], []).pendingSends).toEqual([failed]);
  const retry: OutgoingSend = { ...send, messageId: "b".repeat(32) };
  expect(conversationMessages([retry], []).shown[0].message_id).toBe("bubble");
  expect(conversationMessages([retry], [confirmed]).landed.size).toBe(0);
  const view = conversationMessages([retry], [{ ...confirmed, message_id: retry.messageId }]);
  expect(view.shown).toHaveLength(1);
  expect([...view.landed]).toEqual([["bubble", retry.messageId]]);
});
