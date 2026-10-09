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

test("An unmounted reader leaves the uncancellable read_pty long-poll instead of retaining the terminal", () => {
  assert.match(source, /await Promise\.race\(\[readPtyChunk\(predictedId\), readingStopped\]\)/);
  assert.match(source, /queueMicrotask\(\(\) => \{ invoke<number\[\]>\("read_pty", \{ ptyId \}\)\.then\(resolve, reject\); \}\)/);
  assert.match(source, /await Promise\.race\(\[waitForDrain\(\), readingStopped\]\)/);
  assert.match(source, /reading = false;\r?\n\s+stopReading\(\);/);
});
