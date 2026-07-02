// Run: npx tsx src/components/Sidebar/TabItem.test.ts

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "TabItem.tsx"), "utf8");

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(
  source.includes('import { formatModelBadge } from "../../lib/formatModelBadge";'),
  "TabItem should import the model badge formatter",
);
assert(
  source.includes('import { SidebarModelBadge } from "./SidebarModelBadge";'),
  "TabItem should use the shared sidebar model badge",
);
assert(
  source.includes('import { RoleIcon } from "./HierarchyTree";'),
  "TabItem should use the shared role icon for real orchestrator/worker roles",
);
assert(
  source.includes("const tabModelBadge = tabModel ? formatModelBadge(tabModel) : null;"),
  "TabItem should derive badge text through formatModelBadge",
);
assert(
  source.includes("title={tabModel ?? undefined}"),
  "TabItem should keep the full model id in the badge title",
);
assert(
  source.includes("<SidebarModelBadge label={tabModelBadge}") && !source.includes("{tabModel}\n          </span>"),
  "TabItem should render the formatted badge text, not the raw model id",
);
assert(
  source.includes('data-tab-no-task-bubble="true"'),
  "TabItem should mark tabs as free of task/focus bubble chrome",
);
assert(
  !source.includes("working={status === \"Working\"}"),
  "TabItem should not spin the provider icon while working",
);
assert(
  source.includes("description && !activity"),
  "TabItem should hide description subtitles when plan activity is shown elsewhere",
);
assert(
  source.includes('data-testid="tab-role-indicator"') && source.includes("data-tab-role={pane.role}"),
  "TabItem should expose a role marker tied to explicit pane.role",
);
assert(
  source.includes("pane?.role && !editing") && !source.includes("pane?.role && !activity"),
  "TabItem role marker must not hide when plan activity exists",
);

console.log("✓ TabItem quiet tab chrome: no task bubbles, no working spin, subdued subtitles");
