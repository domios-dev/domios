import assert from "node:assert/strict";
import { test } from "vitest";
import { attachmentMarkerSegments, formatAttachmentMarker as marker, projectTranscript, userMessageAttachments, type ChatSend,
  type MessageSegment, type NormalizedChatMessage } from "@codeterm/chat-core";
import { clearTranscriptWindows, commitTranscriptBatch, readTranscriptWindow } from "./transcriptStore";
import { conversationEntries } from "./chatPagingModel";
import { createDeliveryProjectionMemory, stableDeliveryEntries } from "./chatDeliveryProjection";
import { deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";

const authority = { sourceSessionId: "attachment-session", sessionBindingNonce: "own-launch" };
const at = 1_800_000_000_000;
const image = "C:/chat/phone.jpg";
const other = [marker("C:/chat/clip.mov", "video", "video1"), marker("C:/chat/document.pdf", "file", "document.pdf"),
  marker("C:/chat/unknown.payload", "file", "unknown.payload")];
const original = `Describe: ${[marker(image, "image", "image1"), ...other].join(" ")}`;
const delivered = `Describe: ${[marker(image, "file", "original"), marker("C:/pad/inbox/opaque-model.jpg", "image", "image1"), ...other].join(" ")}`;
const pending: DeliverySend = { id: "composer-send", correlationId: "own-token", content: original, sentAtMs: at,
  echoBoundary: { ...authority, lastRowId: "prior" } };
const confirmed: DeliverySend = { ...pending, status: { send_id: "ledger-send", tab_id: "tab", state: "delivered", error: null,
  ledger_id: "ledger", sequence: 2, retained_from: 1, correlationId: "own-token" }, matchedRow: {
  source: { source_session_id: authority.sourceSessionId, record: { kind: "jsonl", incarnation: "file", record_start: 100, record_row: 0 } }, ordinal: 1,
} };

function echo(id = "provider-uuid", content = delivered, source = authority.sourceSessionId): NormalizedChatMessage {
  const segments: MessageSegment[] = attachmentMarkerSegments(content).map(segment => "text" in segment
    ? { type: "text", text: segment.text }
    : { type: "attachment", kind: segment.attachment.kind, path: segment.attachment.path, label: segment.attachment.label ?? null, body: null, status: null });
  return { id, type: "user", content, timestamp: String(at + 100), sourceSessionId: source, segments };
}

function window(key: string, rows: NormalizedChatMessage[]) {
  clearTranscriptWindows();
  commitTranscriptBatch(key, { kind: "snapshot", authority, range: null, rows: [
    { id: "prior", type: "assistant", content: "before", sourceSessionId: authority.sourceSessionId }, ...rows,
  ] });
  return readTranscriptWindow(key).transcript;
}

test("a model-copy echo replaces one composer bubble with one set of chips and survives reload", () => {
  const key = "model-copy:attachment-session";
  const memory = createDeliveryProjectionMemory(key);
  const empty = window(key, []);
  const before = projectTranscript(empty, deliveryProjectionSends([pending]));
  stableDeliveryEntries(conversationEntries(before.rows), [pending], before.echoes, memory);
  commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows: [echo()] });
  const state = readTranscriptWindow(key).transcript;
  const result = projectTranscript(state, deliveryProjectionSends([confirmed]));
  assert.equal(result.pending.length, 0);
  const rows = stableDeliveryEntries(conversationEntries(result.rows), [confirmed], result.echoes, memory).filter(entry => entry.msg.type === "user");
  assert.equal(rows.length, 1);
  assert.equal(rows[0].msg.id, pending.id);
  assert.equal(rows[0].msg.sourceMessageId, "provider-uuid");
  const chips = userMessageAttachments(rows[0].msg.content).inline?.flatMap(segment => "attachment" in segment ? [segment.attachment.path] : []) ?? [];
  assert.equal(chips.length, 5);
  assert.equal(new Set(chips).size, 5);
  const restored: ChatSend = JSON.parse(JSON.stringify({ ...confirmed, echo: result.echoes.get(pending.id) }));
  const reloaded = projectTranscript(window(key, [echo()]), deliveryProjectionSends([restored]));
  const reloadedRows = stableDeliveryEntries(conversationEntries(reloaded.rows), [restored], reloaded.echoes, createDeliveryProjectionMemory(key));
  assert.equal(reloadedRows.filter(entry => entry.msg.type === "user").length, 1);
  assert.equal(reloadedRows.find(entry => entry.msg.type === "user")?.msg.id, pending.id);
});

test("model-copy reconciliation is single-use and rejects foreign, prior and mismatched attachment rows", () => {
  const key = "model-copy:attachment-session";
  const requests = [pending, { ...pending, id: "second-send", correlationId: "second-token" }];
  const one = projectTranscript(window(key, [echo()]), requests);
  assert.deepEqual(one.pending.map(send => send.id), ["second-send"]);
  const both = projectTranscript(window(key, [echo(), echo("second-provider-uuid")]), requests);
  assert.equal(both.pending.length, 0);
  assert.equal(both.rows.filter(row => row.type === "user").length, 2);
  for (const row of [echo("foreign", delivered, "other-session"), echo("different", delivered.replace("clip.mov", "other.mov"))]) {
    assert.equal(projectTranscript(window(key, [row]), [pending]).pending.length, 1);
  }
  const prior = { ...pending, echoBoundary: { ...pending.echoBoundary!, lastRowId: "provider-uuid" } };
  assert.equal(projectTranscript(window(key, [echo()]), [prior]).pending.length, 1);
});
