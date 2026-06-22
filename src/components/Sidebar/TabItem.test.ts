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

console.log("✓ TabItem applies formatModelBadge and preserves the raw model title");
