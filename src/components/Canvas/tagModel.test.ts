import { facetColorMap, isTagFilterActive, isTagFilterSelected, legendEntries, resolveTags, taskMatchesTagFilter, toggleTagFilter, type TagFilterSelection } from "./tagModel";
import type { TagFacet } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const FACETS: TagFacet[] = [
  {
    key: "area",
    label: "Area",
    color_lane: true,
    values: [{ v: "nav", label: "Navigation", color: "rgba(99,102,241,0.95)" }, { v: "layout" }],
  },
];

test("resolveTags picks first faceted tag as primary and splits topics", () => {
  const r = resolveTags(["area:nav", "pan-zoom", "phase:v3"], FACETS);
  assert(r.primary?.tag === "area:nav", "primary facet tag");
  assert(r.primary?.label === "Navigation", "primary facet label from vocab");
  assert(r.primary?.color === "rgba(99,102,241,0.95)", "primary facet color");
  assert(r.topics.join(",") === "pan-zoom", "topic tags collected");
  assert(r.facets.length === 2, "both faceted tags parsed");
});

test("facetColorMap assigns lane fallback only for color_lane facets", () => {
  const colors = facetColorMap(FACETS);
  assert(colors.get("area:nav") === "rgba(99,102,241,0.95)", "declared color wins");
  assert(colors.get("area:layout")?.includes("rgba"), "fallback lane color for undeclared value");
});

test("legendEntries includes only color_lane facets with resolved colors", () => {
  const legend = legendEntries(FACETS);
  assert(legend.length === 1, "one facet group");
  assert(legend[0].key === "area", "area facet");
  assert(legend[0].values.length === 2, "both values have colors");
});

test("toggleTagFilter selects and clears facet tags per facet bucket", () => {
  let sel: TagFilterSelection = new Map();
  sel = toggleTagFilter(sel, "area:nav");
  assert(isTagFilterSelected(sel, "area:nav"), "nav selected");
  assert(isTagFilterActive(sel), "filter active");
  sel = toggleTagFilter(sel, "area:nav");
  assert(!isTagFilterActive(sel), "cleared when last tag removed");
});

test("taskMatchesTagFilter ORs within facet and ANDs across facets", () => {
  const sel: TagFilterSelection = new Map([["area", new Set(["area:nav", "area:layout"])]]);
  assert(taskMatchesTagFilter(["area:nav"], sel), "nav matches OR bucket");
  assert(taskMatchesTagFilter(["area:layout"], sel), "layout matches OR bucket");
  assert(!taskMatchesTagFilter(["area:storage"], sel), "storage misses OR bucket");

  const multi: TagFilterSelection = new Map([
    ["area", new Set(["area:nav"])],
    ["phase", new Set(["phase:v3"])],
  ]);
  assert(taskMatchesTagFilter(["area:nav", "phase:v3"], multi), "both facets satisfied");
  assert(!taskMatchesTagFilter(["area:nav"], multi), "missing phase fails AND");
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
