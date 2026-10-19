import assert from "node:assert/strict";
import { test } from "node:test";
import { Terminal } from "@xterm/xterm";
import { terminalXtermOptions } from "./terminalXtermOptions";
import { terminalViewerLocalGeometry } from "../../components/Terminal/terminalViewerGeometry";
import { tabOutputRoute } from "../../lib/remoteTabRoute";

const remoteOwnShellTab = { id: "d51b72ef" } as const;
const relayPeerId = "meshqa-peer";

async function write(terminal: Terminal, data: string): Promise<void> {
  await new Promise<void>((resolve) => terminal.write(data, resolve));
}

function bufferRows(terminal: Terminal): string[] {
  const buffer = terminal.buffer.active;
  return Array.from({ length: buffer.length }, (_, index) =>
    buffer.getLine(index)?.translateToString(true) ?? "",
  );
}

test("relay-routed own-shell tabs opt into viewer-local geometry", () => {
  assert.equal("remote_peer_id" in remoteOwnShellTab, false);
  assert.deepEqual(tabOutputRoute(remoteOwnShellTab.id, relayPeerId), {
    peerId: relayPeerId,
    tabId: remoteOwnShellTab.id,
  });
  assert.equal(terminalViewerLocalGeometry(remoteOwnShellTab, relayPeerId), true);
  assert.equal(terminalViewerLocalGeometry(remoteOwnShellTab, null), false);
});

for (const ending of ["", "\r\n"]) {
  for (const length of [163, 193]) {
    test(`relay-routed own shell keeps all ${length} characters after fitting to 45 (${ending ? "hard" : "soft"})`, async () => {
      const geometry = terminalViewerLocalGeometry(remoteOwnShellTab, relayPeerId);
      const content = "x".repeat(length);
      const terminal = new Terminal({
        ...terminalXtermOptions(geometry, 19041),
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
}

test("desktop retains its existing ConPTY renderer options", () => {
  assert.deepEqual(terminalXtermOptions(false, 19041), {
    windowsPty: { backend: "conpty", buildNumber: 19041 },
    reflowCursorLine: false,
  });
});
