import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "Sidebar.tsx"), "utf8");
const creationStart = source.indexOf("const handleNewWorkspace = useCallback");
const creationEnd = source.indexOf("useEffect(() => {", creationStart);
const creationHandler = source.slice(creationStart, creationEnd);
const hydrationStart = source.indexOf("useEffect(() => {", source.indexOf("hydratedProtectedWorkspaceIdsRef"));
const hydrationEnd = source.indexOf("  const handleNewWorkspace", hydrationStart);
const hydrationEffect = source.slice(hydrationStart, hydrationEnd);

assert(creationStart >= 0 && creationEnd > creationStart, "new workspace handler remains discoverable");
assert(!creationHandler.includes("setExpandedWorkspaces"), "creating a workspace never changes disclosure state");
assert(!hydrationEffect.includes("active_workspace_id"), "active workspace changes never expand a workspace");
assert(
  source.match(/setExpandedWorkspaces\(/g)?.length === 2,
  "workspace disclosure state changes only during initial General hydration and explicit user toggles",
);

console.log("workspace collapse ownership source contract passed");
