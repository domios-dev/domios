import { strict as assert } from "node:assert";
import test from "node:test";
import { createTerminalReplyFilter } from "./terminalReplyFilter";

test("drops split OSC color replies and DA replies", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b]10;rgb:c9c9/"), "");
  assert.equal(filter.push("d1d1/d9d9\x1b\\hello\x1b[?1;2c"), "hello");
});

test("drops combined OSC 10 and 11 replies", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(
    filter.push("\x1b]10;rgb:c9c9/d1d1/d9d9\x07\x1b]11;rgb:0d0d/1111/1717\x1b\\ok"),
    "ok",
  );
});

test("drops CPR/DSR cursor position replies", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("a\x1b[12;80Rb"), "ab");
});

test("drops DECRQM replies", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("a\x1b[?2004;1$yb"), "ab");
});

test("passes normal text and key escape sequences", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("literal [?1;2c \x1b[A \x1b[B"), "literal [?1;2c \x1b[A \x1b[B");
});

test("holds escape key until explicit flush", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b"), "");
  assert.equal(filter.flushStandaloneEsc(), "\x1b");
});

test("does not flush incomplete OSC reply as escape", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b]10;rgb:c9c9/"), "");
  assert.equal(filter.flushStandaloneEsc(), "");
  assert.equal(filter.push("d1d1/d9d9\x1b\\ok"), "ok");
});

test("does not flush incomplete DA reply as escape", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b[?"), "");
  assert.equal(filter.flushStandaloneEsc(), "");
  assert.equal(filter.push("1;2cok"), "ok");
});

test("drops OSC reply split immediately after escape", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b"), "");
  assert.equal(filter.push("]10;rgb:c9c9/d1d1/d9d9\x1b\\"), "");
});

test("drops DA reply split immediately after escape", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b"), "");
  assert.equal(filter.push("[?1;2c"), "");
});

test("reset drops stale partial replies", () => {
  const filter = createTerminalReplyFilter();
  assert.equal(filter.push("\x1b]10;rgb:c9c9/"), "");
  filter.reset();
  assert.equal(filter.push("new session text"), "new session text");
});
