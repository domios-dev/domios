import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const here = dirname(fileURLToPath(import.meta.url));
const app = readFileSync(join(here, "../../App.tsx"), "utf8");
const terminalPane = readFileSync(join(here, "TerminalPane.tsx"), "utf8");
const paneSessionChat = readFileSync(join(here, "PaneSessionChat.tsx"), "utf8");

assert(!app.includes("shouldMountTab"), "App must not lazy-mount open tabs");
assert(!app.includes("prewarmTabIds"), "App must not limit mounted tabs to a prewarm window");
assert(
  !/\{uiMode === "chat" && \(\s*<div/.test(terminalPane),
  "TerminalPane must keep Chat mounted while Terminal mode is visible",
);
assert(
  /useChatSession\(\{[\s\S]*?active: true,/.test(paneSessionChat),
  "mounted Chat must keep its transcript subscription active while hidden",
);

console.log("✓ open tabs and Chat subscriptions stay eagerly mounted");
