import assert from "node:assert/strict";
import { test } from "node:test";
import { Terminal } from "@xterm/xterm";
import { terminalXtermOptions } from "./terminalXtermOptions";

async function write(terminal: Terminal, data: string): Promise<void> {
  await new Promise<void>((resolve) => terminal.write(data, resolve));
}

function bufferRows(terminal: Terminal): string[] {
  const buffer = terminal.buffer.active;
  return Array.from({ length: buffer.length }, (_, index) =>
    buffer.getLine(index)?.translateToString(true) ?? "",
  );
}

for (const ending of ["", "\r\n"]) {
  test(`remote viewer keeps every character in a 163-column line after fitting to 45 (${ending ? "hard" : "soft"})`, async () => {
    const content = `${"x".repeat(163)}${ending ? "" : "y"}`;
    const terminal = new Terminal({
      ...terminalXtermOptions(true, 19041),
      cols: 163,
      rows: 8,
      scrollback: 100,
    });

    try {
      await write(terminal, `${content}${ending}`);
      terminal.resize(45, 8);

      assert.equal(terminal.cols, 45);
      const rows = bufferRows(terminal);
      assert.ok(rows.every((row) => row.length <= 45));
      assert.equal(rows.join(""), content);
    } finally {
      terminal.dispose();
    }
  });
}

test("desktop retains its existing ConPTY renderer options", () => {
  assert.deepEqual(terminalXtermOptions(false, 19041), {
    windowsPty: { backend: "conpty", buildNumber: 19041 },
    reflowCursorLine: false,
  });
});
