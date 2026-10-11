import { describe, expect, it } from "vitest";
import { attachmentActivity, attachmentStateForSnapshot, canAdoptTranscriptAuthority, needsTranscriptAttachment, transcriptAuthorityMatches, type ChatStartResult } from "./chatSessionAuthority";
import { createTranscriptState, reduceTranscript, projectTranscript } from "@codeterm/chat-core";
import { deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";

describe("bound file-tail attachment", () => {
  it.each([
    { messages: [] },
    { messages: [{ id: "row", type: "assistant" as const, content: "answer", timestamp: "1" }] },
  ])(
    "completes a non-pending snapshot independently of row count: %j",
    ({ messages }) => {
      const snapshot: ChatStartResult = {
        source_session_id: "own-session",
        session_binding_nonce: "own-binding",
        text: "",
        offset: 0,
        pending: false,
        messages,
      };
      expect(attachmentActivity("awaiting_store", false, true).recovering).toBe(true);
      const state = attachmentStateForSnapshot(snapshot);
      expect(state).toBe("attached");
      expect(attachmentActivity(state, true, true)).toEqual({ loading: false, recovering: false });
    },
  );

  it("keeps a pending store in recovery until observed readiness", () => {
    const state = attachmentStateForSnapshot({ pending: true });
    expect(state).toBe("awaiting_store");
    expect(attachmentActivity(state, false, true)).toEqual({ loading: false, recovering: true });
    expect(attachmentActivity("attached", false, true)).toEqual({ loading: false, recovering: false });
  });
});


it("a fresh unbound launch does not recover the previous session transcript", () => {
  expect(attachmentActivity("awaiting_session", false, true, false)).toEqual({ loading: false, recovering: false });
  expect(attachmentActivity("awaiting_store", false, true, true)).toEqual({ loading: false, recovering: true });
});

it.each([false, true])("a fresh session requires its own listener and renders live turns with held sends=%s", (held) => {
  const old = { source_session_id: "previous", session_binding_nonce: "previous-nonce" };
  const fresh = { source_session_id: "fresh", session_binding_nonce: "fresh-nonce" };
  expect(transcriptAuthorityMatches(old, null, "fresh-nonce")).toBe(false);
  expect(transcriptAuthorityMatches(old, "fresh", "fresh-nonce")).toBe(false);
  expect(transcriptAuthorityMatches(null, "fresh", "fresh-nonce")).toBe(false);
  expect(transcriptAuthorityMatches(fresh, "fresh", "fresh-nonce")).toBe(true);
  expect(canAdoptTranscriptAuthority(old, "fresh", "fresh-nonce")).toBe(false);
  expect(canAdoptTranscriptAuthority(fresh, "fresh", "fresh-nonce")).toBe(true);
  const authority = { sourceSessionId: "fresh", sessionBindingNonce: "fresh-nonce" };
  const snapshot = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority,
    range: { start: 0, end: 1 }, rows: [{ id: "seed", type: "user", content: "opaque-seed" }] }).state;
  const live = reduceTranscript(snapshot, { kind: "delta", authority, range: { start: 1, end: 2 },
    rows: [{ id: "answer", type: "assistant", content: "opaque-answer" }] }).state;
  const sends: DeliverySend[] = held ? ["one", "two"].map((id, sequence) => ({
    id, correlationId: id, content: id, sentAtMs: sequence,
    status: { send_id: id, correlationId: id, tab_id: "tab", ledger_id: "ledger", sequence,
      retained_from: 0, state: "failed", error: "launch_ended_before_write" },
  })) : [];
  const projected = projectTranscript(live, deliveryProjectionSends(sends));
  expect(projected.rows.some((row) => row.content === "opaque-answer")).toBe(true);
  expect(projected.pending.map((send) => send.id)).toEqual(held ? ["one", "two"] : []);
});

it("an authority observed while attachment is paused remains owed until a request starts", () => {
  const requested = { source_session_id: "old", session_binding_nonce: "old-nonce" };
  expect(needsTranscriptAttachment(requested, "fresh", "fresh-nonce", true, false)).toBe(false);
  expect(needsTranscriptAttachment(requested, "fresh", "fresh-nonce", false, true)).toBe(false);
  expect(needsTranscriptAttachment(requested, "fresh", "fresh-nonce", true, true)).toBe(true);
  const freshRequest = { source_session_id: "fresh", session_binding_nonce: "fresh-nonce" };
  expect(needsTranscriptAttachment(freshRequest, "fresh", "fresh-nonce", true, true)).toBe(false);
  expect(needsTranscriptAttachment(freshRequest, "fresh", "next-nonce", true, true)).toBe(true);
});
