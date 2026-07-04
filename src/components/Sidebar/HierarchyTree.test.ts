// Run: npx tsx src/components/Sidebar/HierarchyTree.test.ts

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "HierarchyTree.tsx"), "utf8");
const css = readFileSync(join(dirname(dirname(here)), "styles", "globals.css"), "utf8");

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(
  !source.includes('import { StatusDot }'),
  "HierarchyTree must not import StatusDot on subagent rows",
);
assert(
  !source.includes("<StatusDot"),
  "HierarchyTree must not render StatusDot",
);
assert(
  source.includes('data-hierarchy-status='),
  "HierarchyTree rows expose data-hierarchy-status",
);
assert(
  source.includes('data-hierarchy-working='),
  "HierarchyTree rows expose data-hierarchy-working",
);
assert(
  source.includes('data-hierarchy-dead='),
  "HierarchyTree rows expose data-hierarchy-dead for dead panes",
);
assert(
  source.includes('data-hierarchy-task-line="true"'),
  "HierarchyTree exposes task line marker",
);
assert(
  source.includes("activeWorkForPane"),
  "HierarchyTree binds task line via activeWorkForPane",
);
assert(
  source.includes("sortHierarchyNodes"),
  "HierarchyTree uses sortHierarchyNodes for calm reorder",
);
assert(
  source.includes("sidebar-hierarchy-working--aurora") &&
    source.includes("sidebar-hierarchy-working--drift") &&
    source.includes("sidebar-hierarchy-working--halo"),
  "HierarchyTree prototypes three working-row CSS variants",
);
assert(
  !source.includes('node.pane.role === "Worker"') || source.includes("isWorker"),
  "HierarchyTree distinguishes worker rows for task line",
);
assert(
  source.includes('data-hierarchy-dead-mark="true"'),
  "Dead rows use trailing mark instead of StatusDot",
);
assert(
  css.includes("@media (prefers-reduced-motion: reduce)") &&
    css.includes(".sidebar-hierarchy-working--aurora"),
  "globals.css provides reduced-motion overrides for hierarchy animations",
);
assert(
  !source.includes("border-l border-[var(--ct-accent)]"),
  "HierarchyTree must not add colored accent stripes on working rows",
);
assert(
  source.includes("excludePaneIds"),
  "HierarchyTree buildHierarchySubtree accepts excludePaneIds for attached Planners",
);

console.log("✓ HierarchyTree: row states, task line, sort, variants, no StatusDot clutter");
