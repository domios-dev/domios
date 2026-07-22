import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "Sidebar.tsx"), "utf8");
const workspaceItemSource = readFileSync(join(here, "WorkspaceItem.tsx"), "utf8");
const start = source.indexOf("const handleNewSession = useCallback");
const end = source.indexOf("  const [expandedWorkspaces", start);
const handler = source.slice(start, end);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(start >= 0 && end > start, "Sidebar keeps the new session handler");
assert(handler.includes("setSessionPicker({ workspaceId, workspaceCwd });"), "new session handler opens the modal picker");
assert(
  handler.includes("}, []);"),
  "modal picker opening does not depend on tab navigation",
);
assert(!handler.includes("onNavigateToTab"), "new session leaves the current tab visible behind the modal");
assert(workspaceItemSource.includes('label: "New session"'), "workspace actions expose the full session picker");
assert(workspaceItemSource.includes('label: "Quick spawn"'), "workspace actions group fast provider starts in a submenu");
assert(workspaceItemSource.includes('description: "Spawns a default Worker"'), "Quick spawn describes its default role");
for (const provider of ["Codex", "Claude Code", "OpenCode", "Cursor"]) {
  assert(workspaceItemSource.includes(`label: "${provider}"`), `Quick spawn exposes ${provider}`);
}

console.log("new session modal source contract passed");
