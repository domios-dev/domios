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
  source.includes('import { RoleModelBadge } from "./RoleModelBadge";') &&
    source.includes('import { roleBadgeLabel } from "./orchestrationSections";'),
  "TabItem should use the leadership role+model badge + shared role→section mapping (star deleted)",
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
  source.includes("const roleBadge = roleBadgeLabel(pane);") &&
    source.includes("<RoleModelBadge role={roleBadge}"),
  "TabItem should show a leadership role+model badge (ORCHESTRATOR/PLANNER · MODEL) in the model-chip slot",
);
assert(
  !source.includes("RoleIcon") &&
    !source.includes('tab-role-indicator') &&
    !source.includes("★"),
  "TabItem must not render the deleted role star / icon",
);
assert(
  source.includes("roleBadge ? (") && source.includes("tabModelBadge && !touchMode ?"),
  "Role badge replaces the model chip only for leadership; workers keep the model chip",
);

assert(
  source.includes("pp.title === np.title"),
  "TabItem memo must compare pane.title for rename re-renders",
);
assert(
  source.includes('data-testid="tab-pane-icon"'),
  "TabItem exposes a left provider icon marker for child/manager rows",
);

console.log("✓ TabItem quiet tab chrome: no task bubbles, no working spin, subdued subtitles");
