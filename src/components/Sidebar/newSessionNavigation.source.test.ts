import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "Sidebar.tsx"), "utf8");
const start = source.indexOf("const handleNewSession = useCallback");
const end = source.indexOf("  const [expandedWorkspaces", start);
const handler = source.slice(start, end);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const openPicker = handler.indexOf("openSessionPicker(workspaceId, workspaceCwd ?? undefined);");
const navigate = handler.indexOf("onNavigateToTab?.();");

assert(start >= 0 && end > start, "Sidebar keeps the new session handler");
assert(openPicker >= 0, "new session handler opens the picker");
assert(navigate > openPicker, "new session navigation follows picker opening");
assert(
  handler.includes("}, [openSessionPicker, onNavigateToTab]);"),
  "new session handler tracks navigation callback dependency",
);
assert(!/\bif\s*\(/.test(handler), "new session navigation has no drawer/picker occlusion exemption");

console.log("new session navigation source contract passed");
