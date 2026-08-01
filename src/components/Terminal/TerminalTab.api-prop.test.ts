import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const here = dirname(fileURLToPath(import.meta.url));
const terminalTab = readFileSync(join(here, "TerminalTab.tsx"), "utf8");
const tabContent = readFileSync(join(here, "TabContent.tsx"), "utf8");

assert(
  /api\?: ApiConfig;/.test(terminalTab),
  "TerminalTabProps must accept ApiConfig so PTY chat can resolve plan refs",
);
assert(
  /<TabSessionChat[\s\S]*tabId=\{tabId\}[\s\S]*api=\{api\}/.test(terminalTab),
  "TerminalTab must pass api into TabSessionChat for PTY chat mode",
);
assert(
  /<TerminalTab[\s\S]*api=\{api\}/.test(tabContent),
  "TabContent must pass api into TerminalTab",
);

console.log("✓ TerminalTab forwards api to PTY chat plan-ref resolver");
