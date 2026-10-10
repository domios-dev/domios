// Run: npx tsx src/components/Surface/sseParser.test.ts

import assert from "node:assert/strict";
import { createSseParser, type SseMessage } from "./sseParser";

function parseAll(chunks: string[]): SseMessage[] {
  const parser = createSseParser();
  return chunks.flatMap((chunk) => parser.push(chunk));
}

assert.deepEqual(parseAll(["event: state\ndata: {\"a\":1}\n\n"]), [{ event: "state", data: "{\"a\":1}" }], "a named event with one data line");

assert.deepEqual(parseAll(["data: hello\n\n"]), [{ event: "message", data: "hello" }], "an unnamed event defaults to message");

assert.deepEqual(parseAll(["data: one\ndata: two\ndata:three\n\n"]), [{ event: "message", data: "one\ntwo\nthree" }], "multi-line data joins with LF and strips only one leading space");

assert.deepEqual(parseAll(["data:  spaced\n\n"]), [{ event: "message", data: " spaced" }], "only the first space after the colon is dropped");

const whole = "event: frame\ndata: {\"data\":\"QUJD\"}\n\nevent: notice\ndata: {\"text\":\"hi\"}\n\n";
const expected = [{ event: "frame", data: "{\"data\":\"QUJD\"}" }, { event: "notice", data: "{\"text\":\"hi\"}" }];
for (let size = 1; size <= whole.length; size += 1) {
  const chunks: string[] = [];
  for (let index = 0; index < whole.length; index += size) chunks.push(whole.slice(index, index + size));
  assert.deepEqual(parseAll(chunks), expected, `chunk size ${size} yields the same events`);
}

assert.deepEqual(parseAll(["event: state\r\ndata: x\r\n\r\n"]), [{ event: "state", data: "x" }], "CRLF line endings");
assert.deepEqual(parseAll(["event: state\r", "\ndata: x\r", "\n\r", "\n"]), [{ event: "state", data: "x" }], "a CRLF split across chunks is one line break");
assert.deepEqual(parseAll(["data: x\r\r"]), [{ event: "message", data: "x" }], "bare CR line endings");
assert.deepEqual(parseAll(["data: a\r", "data: b\r\r"]), [{ event: "message", data: "a\nb" }], "a CR at a chunk end followed by a new line is not swallowed");

assert.deepEqual(parseAll([": keep-alive\n\n", "data: x\n\n"]), [{ event: "message", data: "x" }], "comments and empty events dispatch nothing");
assert.deepEqual(parseAll(["event: state\n\n"]), [], "an event without data is dropped");
assert.deepEqual(parseAll(["id: 7\nretry: 100\nfoo: bar\ndata: x\n\n"]), [{ event: "message", data: "x", id: "7" }], "id is kept, unknown fields ignored");
assert.deepEqual(parseAll(["﻿data: x\n\n"]), [{ event: "message", data: "x" }], "a leading BOM is stripped");
assert.deepEqual(parseAll(["data\n\n"]), [{ event: "message", data: "" }], "a field without a colon has an empty value");

assert.deepEqual(parseAll(["event: state\ndata: half"]), [], "an unterminated event is held back");
const parser = createSseParser();
assert.deepEqual(parser.push("event: state\ndata: {\"owner\":"), []);
assert.deepEqual(parser.push("\"Jev\"}\n"), []);
assert.deepEqual(parser.push("\n"), [{ event: "state", data: "{\"owner\":\"Jev\"}" }], "a data line spanning chunks is reassembled");
assert.deepEqual(parser.push("event: notice\ndata: y\n\n"), [{ event: "notice", data: "y" }], "the event type resets between events");

console.log("✓ sseParser tests green");
