import { stressRows, stressRange, stressAuthority, stressReplyIds, stressStart, stressBackfill } from "../../../tests/fixtures/chat/imageStress";
import { expect, test } from "vitest";
import { plain, recordInput } from "../../../plugins/provider-shared/tests/recordHookFixture";
import { parser, raw, lines, authority, noticeId, ownerId, flatten, parsed, rows, end, owner } from "../../../tests/fixtures/chat/airTranscript";
import { captureSendEchoBoundary, type ChatSend } from "./sendOrder";
import { createTranscriptState, expandTranscriptGap, projectTranscript, reduceTranscript, transcriptRows, type TranscriptState } from "./transcript";
import type { NormalizedChatMessage } from "./types";

function batch(state: TranscriptState, kind: "snapshot" | "delta" | "backfill", rows: NormalizedChatMessage[], start: number, end: number) {
  return reduceTranscript(state, { kind, authority, range: { start, end }, rows }).state;
}

const snapshot = () => batch(createTranscriptState(), "snapshot", rows, 0, end);

test("air parser keeps every assistant and rejects request copies in text and record paths", () => {
  const input = recordInput(raw);
  expect(plain(parser.parseSessionRecords(input.records, input.meta).messages)).toEqual(parsed);
  const expected = lines.map(line => JSON.parse(line) as { type: string; uuid?: string })
    .filter(row => row.type === "assistant").map(row => row.uuid);
  expect(parsed.filter(row => row.role === "assistant").map(row => row.uuid)).toEqual(expected);
  expect(parsed.filter(row => row.uuid == null).every(row => row.blocks.length === 0)).toBe(true);
});

test("air snapshot and record-by-record live replay preserve the same assistant IDs", () => {
  let live = createTranscriptState();
  let offset = 0;
  for (const line of lines) {
    const next = offset + Buffer.byteLength(`${line}\n`);
    live = batch(live, "delta", flatten(plain(parser.parseSessionDelta(line).messages)), offset, next);
    offset = next;
  }
  expect(transcriptRows(live).map(row => row.id)).toEqual(transcriptRows(snapshot()).map(row => row.id));
});

test("air queued notice stays at its own time across snapshot and live growth", () => {
  const state = snapshot();
  const projected = projectTranscript(state).rows;
  const notice = projected.findIndex(row => row.id === noticeId);
  const laterResult = projected.findIndex(row => row.id === "result-toolu_014F1x7QYGnVzVrUTvS7KjBv");
  expect(notice).toBeGreaterThanOrEqual(0);
  expect(notice).toBeLessThan(laterResult);
  const grown = batch(state, "delta", [{ ...owner, id: "next-turn" }], end, end + 100);
  expect(projectTranscript(grown).rows.filter(row => row.id === noticeId)).toHaveLength(1);
  expect(projectTranscript(grown).rows.findIndex(row => row.id === noticeId)).toBe(notice);
});

function acceptedSend(): ChatSend {
  const boundary = { ...captureSendEchoBoundary(authority, rows.slice(0, rows.indexOf(owner)))!,
    endOffset: end - 100 };
  return { id: "air-send", correlationId: "air-send", content: owner.content,
    sentAtMs: Date.parse(owner.timestamp!), echoBoundary: boundary };
}

test("air send with an exact echo token renders once after the captured boundary leaves the reload window", () => {
  const send = JSON.parse(JSON.stringify(acceptedSend())) as ChatSend;
  const reloaded = batch(createTranscriptState(), "snapshot", [{ ...owner, correlationId: send.correlationId }], end, end + 100);
  expect(projectTranscript(reloaded, [send]).rows.map(row => row.id)).toEqual([ownerId]);
});

test("air untokened echo arriving after reload uses the retained source boundary", () => {
  const send = JSON.parse(JSON.stringify(acceptedSend())) as ChatSend;
  let reloaded = batch(createTranscriptState(), "snapshot", [], end, end + 100);
  expect(projectTranscript(reloaded, [send]).pending).toHaveLength(1);
  reloaded = batch(reloaded, "delta", [owner], end + 100, end + 200);
  expect(projectTranscript(reloaded, [send]).rows.map(row => row.id)).toEqual([ownerId]);
});

test("air history pages and expanded gaps never duplicate a queued notice", () => {
  const cut = Math.floor(rows.length / 2);
  let state = batch(createTranscriptState(), "snapshot", rows.slice(cut), 100, end);
  state = batch(state, "backfill", rows.slice(0, cut), 0, 100);
  expect(transcriptRows(state).map(row => row.id)).toEqual(rows.map(row => row.id));
  const gap: NormalizedChatMessage = { id: "air-gap", type: "system", content: "", timelineMarker: {
    kind: "history_gap", source: "provider_session", session_id: authority.sourceSessionId, before_offset: 100,
  } };
  let gapped = batch(createTranscriptState(), "snapshot", [gap, ...rows.slice(cut)], 100, end);
  gapped = expandTranscriptGap(gapped, gap.id, rows.slice(0, cut)).state;
  gapped = batch(gapped, "delta", [gap, ...rows.slice(cut)], end, end + 100);
  expect(transcriptRows(gapped).some(row => row.id === gap.id)).toBe(false);
  expect(transcriptRows(gapped).filter(row => row.id === noticeId)).toHaveLength(1);
});

test("air overlapping live window updates a known notice in place while recovering missing assistant rows", () => {
  const notice = rows.find(row => row.id === noticeId)!;
  const earlier = rows.find(row => row.type === "tool_result")!;
  const assistant = rows.find(row => row.type === "assistant")!;
  const after = rows.find(row => row.id === ownerId)!;
  const initial = batch(createTranscriptState(), "snapshot", [earlier, notice, after], 0, 100);
  const replay = batch(initial, "delta", [notice, assistant], 50, 200);
  expect(transcriptRows(replay).map(row => row.id)).toEqual([earlier.id, notice.id, after.id, assistant.id]);
  expect(projectTranscript(replay).rows.filter(row => row.id === noticeId)).toHaveLength(1);
});

test("air windowed history cannot acknowledge an untokened pre-send identical row", () => {
  const send = acceptedSend();
  const old = batch(createTranscriptState(), "snapshot", [owner], 0, end - 101);
  expect(projectTranscript(old, [send]).pending).toHaveLength(1);
  const foreign = batch(createTranscriptState(), "snapshot", [{ ...owner, sourceSessionId: "foreign" }], end, end + 100);
  expect(projectTranscript(foreign, [send]).pending).toHaveLength(1);
  const rebound = { ...foreign, authority: { ...authority, sessionBindingNonce: "new-launch" } };
  expect(projectTranscript(rebound, [send]).pending).toHaveLength(1);
});

test("air replayed old identity cannot acquire a newer append boundary", () => {
  const send = acceptedSend();
  const retained = [{ ...rows[0] }, owner];
  let state = batch(createTranscriptState(), "snapshot", retained, 0, end - 101);
  state = batch(state, "delta", [], end - 101, end + 1);
  state = batch(state, "delta", [owner, { ...rows[0], id: "later-row" }], end + 1, end + 100);
  expect(projectTranscript(state, [send]).pending).toHaveLength(1);
});
test("stress cached snapshot keeps reply order after older byte pages arrive", () => {
  const cached = [...stressRows(85, 96), ...stressRows(97, 104)];
  const older = stressRows(77, 84);
  let state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority: stressAuthority, range: stressRange(85, 104), rows: cached,
  }).state;
  expect(transcriptRows(state).filter(row => stressReplyIds.includes(row.id)).map(row => row.id)).toEqual(stressReplyIds);
  state = reduceTranscript(state, { kind: "backfill", authority: stressAuthority,
    range: stressRange(77, 84), rows: older }).state;
  expect(transcriptRows(state).filter(row => stressReplyIds.includes(row.id)).map(row => ({ id: row.id, seq: row.seq })))
    .toEqual(cached.filter(row => stressReplyIds.includes(row.id)).map(row => ({ id: row.id, seq: row.seq })));
});

test("stress actual cached start and empty older page preserve source reply order", () => {
  const authority = { sourceSessionId: stressStart.source_session_id, sessionBindingNonce: stressStart.session_binding_nonce };
  let state = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority,
    range: { start: stressStart.start_offset, end: stressStart.offset, unit: stressStart.offset_kind }, rows: stressStart.messages }).state;
  const before = transcriptRows(state);
  expect(before.filter(row => stressReplyIds.includes(row.id)).map(row => row.id)).toEqual(stressReplyIds);
  state = reduceTranscript(state, { kind: "backfill", authority,
    range: { start: stressBackfill.start_offset, end: stressBackfill.end_offset, unit: stressBackfill.offset_kind }, rows: stressBackfill.messages }).state;
  expect(transcriptRows(state)).toEqual(before);
});
