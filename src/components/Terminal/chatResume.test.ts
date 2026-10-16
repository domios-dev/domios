import assert from "node:assert/strict";
import { test } from "node:test";
import { isByteResumeCursor, resumeChatCursor, type ChatResumeDelta } from "./chatResume";

const delta = (cursor: string): ChatResumeDelta => ({ messages: [], start_offset: 1, offset: 2,
  source_session_id: "supplied-session", session_binding_nonce: "supplied-nonce", cursor });

test("caught-up resume carries no transcript and never requests attachment", async () => {
  let reads = 0;
  const outcome = await resumeChatCursor("supplied-cursor", async cursor => {
    assert.equal(cursor, "supplied-cursor");
    reads++;
    return { kind: "resumed", deltas: [], caught_up: true };
  }, () => assert.fail("idle resume must not replace the projection"), () => true);
  assert.equal(outcome, "caught_up");
  assert.equal(reads, 1);
});

test("bounded pages advance only through response cursors and merge once", async () => {
  const reads: string[] = [];
  const adopted: string[] = [];
  const outcome = await resumeChatCursor("a", async cursor => {
    reads.push(cursor);
    return { kind: "resumed", deltas: [delta(cursor === "a" ? "b" : "c")], caught_up: cursor === "b" };
  }, row => adopted.push(row.cursor), () => true);
  assert.equal(outcome, "caught_up");
  assert.deepEqual(reads, ["a", "b"]);
  assert.deepEqual(adopted, ["b", "c"]);
});

test("reset and unsupported retain explicit cold-attachment fallback", async () => {
  for (const result of [{ kind: "reset_required", reason: "compacted" },
    { kind: "unsupported", source_kind: "items" }] as const) {
    assert.equal(await resumeChatCursor("a", async () => result, () => assert.fail(), () => true), "attach");
  }
});

test("a non-advancing continuation fails rather than looping", async () => {
  await assert.rejects(resumeChatCursor("a", async () => ({ kind: "resumed", deltas: [delta("a")], caught_up: false }),
    () => {}, () => true));
});

test("authority replacement discards an in-flight response", async () => {
  let current = true;
  const outcome = await resumeChatCursor("a", async () => {
    current = false;
    return { kind: "resumed", deltas: [delta("b")], caught_up: true };
  }, () => assert.fail("foreign authority cannot adopt"), () => current);
  assert.equal(outcome, "superseded");
});

test("only a typed byte cursor enables the JSONL resume slice", () => {
  assert.equal(isByteResumeCursor(JSON.stringify({ kind: "bytes", incarnation: "supplied", offset: 20 })), true);
  for (const value of [null, "provider-opaque", '{"kind":"items","count":20}', '{"kind":"bytes","offset":-1}']) {
    assert.equal(isByteResumeCursor(value), false);
  }
});
