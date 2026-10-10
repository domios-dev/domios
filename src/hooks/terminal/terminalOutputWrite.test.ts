import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";
import type { Terminal as Xterm } from "@xterm/xterm";
import { SHELL_RESTORE_VIEWPORT_MARKER, writeTerminalOutput } from "./terminalOutputWrite";
import { writeTerminalHistory } from "./terminalHistoryWrite";

Object.defineProperty(globalThis, "self", { value: globalThis, configurable: true });
const { Terminal } = createRequire(import.meta.url)("@xterm/xterm") as typeof import("@xterm/xterm");

function output(term: Xterm, data: string | Uint8Array): Promise<void> {
  return new Promise((resolve) => writeTerminalOutput(term, data, resolve));
}

function history(term: Xterm, data: string): Promise<void> {
  return new Promise((resolve) => { writeTerminalHistory(term, data, resolve); });
}

function lines(term: Xterm): string[] {
  const buffer = term.buffer.active;
  const result = Array.from({ length: buffer.length }, (_, row) => buffer.getLine(row)!.translateToString(true));
  while (result[result.length - 1] === "") result.pop();
  return result;
}

test("restored shell history precedes a clean cursor-addressed prompt at the actual viewer size", async () => {
  for (const rows of [6, 24, 40]) {
    for (const count of [4, 24, 50]) {
      const term = new Terminal({ cols: 80, rows, scrollback: 256 });
      try {
        const saved = Array.from({ length: count }, (_, index) => `saved-${index}`);
        const replay = history(term, saved.join("\r\n") + SHELL_RESTORE_VIEWPORT_MARKER);
        const firstFrame = output(term, "\x1b[HPS> ");
        await Promise.all([replay, firstFrame]);
        assert.equal(term.options.scrollOnEraseInDisplay, false);
        assert.equal(term.buffer.active.getLine(term.buffer.active.baseY)!.translateToString(true), "PS> ");
        assert.equal(term.buffer.active.cursorY, 0);
        await output(term, "фыва");
        assert.deepEqual(lines(term), [...saved, "PS> фыва"]);
      } finally {
        term.dispose();
      }
    }
  }
});

test("repeated shell restores retain every prior frame once without blank padding or overwritten prompts", async () => {
  let saved = "saved-0\r\nsaved-1\r\n";
  const expected = ["saved-0", "saved-1"];
  for (const [epoch, rows] of [6, 40, 24].entries()) {
    const term = new Terminal({ cols: 80, rows, scrollback: 256 });
    try {
      saved += SHELL_RESTORE_VIEWPORT_MARKER;
      await history(term, saved);
      const firstFrame = `\x1b[Hbanner-${epoch}\r\nPS> input-${epoch}`;
      await output(term, firstFrame);
      saved += firstFrame;
      expected.push(`banner-${epoch}`, `PS> input-${epoch}`);
      assert.deepEqual(lines(term), expected);
    } finally {
      term.dispose();
    }
  }
});

test("a shell restore marker split across transport chunks still precedes the first live frame", async () => {
  const term = new Terminal({ cols: 80, rows: 6 });
  try {
    const marker = new TextEncoder().encode(SHELL_RESTORE_VIEWPORT_MARKER);
    await output(term, "saved-0\r\nsaved-1");
    for (const byte of marker) await output(term, Uint8Array.of(byte));
    await output(term, "\x1b[HPS> ");
    assert.deepEqual(lines(term), ["saved-0", "saved-1", "PS> "]);
  } finally {
    term.dispose();
  }
});

test("agent history replay and full-screen repaint retain the existing viewport and scrollback", async () => {
  const term = new Terminal({ cols: 80, rows: 6 });
  try {
    await history(term, "\x1b[?1049h\x1b[Hprevious frame");
    await output(term, "\x1b[2J\x1b[Hnew frame\x1b[6;1Hcomposer> ");
    assert.equal(term.buffer.active.type, "alternate");
    assert.equal(term.buffer.active.baseY, 0);
    assert.equal(term.buffer.active.cursorY, 5);
    assert.deepEqual(lines(term), ["new frame", "", "", "", "", "composer> "]);
    assert.equal(term.buffer.normal.baseY, 0);
    assert.equal(term.buffer.normal.length, 6);
  } finally {
    term.dispose();
  }
});

test("ordinary OSC output and partial non-marker escape sequences pass through unchanged", async () => {
  const writes: number[] = [];
  const term = { write(data: string | Uint8Array, done?: () => void) {
    writes.push(...(typeof data === "string" ? new TextEncoder().encode(data) : data));
    done?.();
  } };
  const payload = "\x1b]0;title\x07text\x1b[31mred\x1b[0m";
  for (const byte of new TextEncoder().encode(payload)) await new Promise<void>((resolve) => {
    writeTerminalOutput(term, Uint8Array.of(byte), resolve);
  });
  assert.deepEqual(writes, Array.from(new TextEncoder().encode(payload)));
});
