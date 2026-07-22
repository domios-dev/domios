import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const here = dirname(fileURLToPath(import.meta.url));
const terminalPane = readFileSync(join(here, "TerminalPane.tsx"), "utf8");
const tabContent = readFileSync(join(here, "TabContent.tsx"), "utf8");

assert(
  /api\?: ApiConfig;/.test(terminalPane),
  "TerminalPaneProps must accept ApiConfig so PTY chat can resolve plan refs",
);
assert(
  /<PaneSessionChat[\s\S]*paneId=\{paneId\}[\s\S]*api=\{api\}/.test(terminalPane),
  "TerminalPane must pass api into PaneSessionChat for PTY chat mode",
);
assert(
  /<TerminalPane[\s\S]*api=\{api\}/.test(tabContent),
  "TabContent must pass api into TerminalPane",
);

console.log("✓ TerminalPane forwards api to PTY chat plan-ref resolver");
