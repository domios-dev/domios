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
  handler.includes("if (firstRunActive) return;") && handler.includes("}, [firstRunActive]);"),
  "Get started blocks the modal at the handler boundary",
);
assert(source.includes("if (firstRunActive) setSessionPicker(null);"), "Get started closes an already-open picker");
assert(!handler.includes("onNavigateToTab"), "new session leaves the current tab visible behind the modal");
assert(
  source.includes("Object.values(state.tabs).some(isFirstRunGeneralTab)"),
  "Get started gate is workspace-independent",
);
assert(
  (source.match(/newSessionDisabled=\{firstRunActive\}/g) ?? []).length === 3,
  "Get started gate covers General, workspace rows, and workspace menus",
);
assert(workspaceItemSource.includes('label: "New session"'), "workspace actions expose the full session picker");
assert(workspaceItemSource.includes("disabled: newSessionDisabled"), "workspace menu honors the Get started gate");
assert(workspaceItemSource.includes("onClick: newSessionDisabled ? undefined : onNewSession"), "disabled workspace menu cannot create a session");
assert(workspaceItemSource.includes('label: "Quick spawn"'), "workspace actions group fast provider starts in a submenu");
assert(
  workspaceItemSource.includes('label: "Quick spawn",')
    && workspaceItemSource.includes("disabled: newSessionDisabled"),
  "Get started disables Quick spawn",
);
assert(workspaceItemSource.includes('description: "Spawns a default Worker"'), "Quick spawn describes its default role");
for (const provider of ["Codex", "Claude Code", "OpenCode", "Cursor"]) {
  assert(workspaceItemSource.includes(`label: "${provider}"`), `Quick spawn exposes ${provider}`);
}

console.log("new session modal source contract passed");
