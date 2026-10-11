import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const source = readFileSync(join(process.cwd(), "src/hooks/terminal/usePtyConnection.ts"), "utf8");

test("Tab cwd updates do not restart the live PTY reader", () => {
  assert.match(source, /const tabCwdRef = useRef\(tabCwd\)/);
  assert.doesNotMatch(source, /\}, \[ptyId, tabId, tabCwd,/);
  assert.match(source, /consumePendingCwd\(tabId\) \|\| tabCwdRef\.current/);
});

test("Unmount detaches the stream without polling or killing a shared PTY", () => {
  assert.match(source, /unsubscribe\?\.\(\)/);
  assert.doesNotMatch(source, /read_pty|kill_pty|setTimeout/);
  assert.match(source, /subscribeTabOutput\(tabId, sink, term, id\)/);
});
