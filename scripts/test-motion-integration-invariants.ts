import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { extname, isAbsolute, join, relative, resolve, sep } from "node:path";

const root = resolve(import.meta.dirname, "..");
const isInsideRepository = (path: string) => {
  const pathFromRoot = relative(root, path);
  return (
    pathFromRoot === "" ||
    (!isAbsolute(pathFromRoot) &&
      pathFromRoot !== ".." &&
      !pathFromRoot.startsWith(`..${sep}`))
  );
};
const srcRoot = join(root, "src");
const globalsPath = join(srcRoot, "styles", "globals.css");
const agentStatePath = join(srcRoot, "styles", "agent-state.css");
const stylesRoot = join(srcRoot, "styles");
const activityDotsPath = join(srcRoot, "components", "shared", "ActivityDots.tsx");
const segmentedDonutPath = join(srcRoot, "components", "shared", "SegmentedDonut.tsx");
const globalsCss = readFileSync(globalsPath, "utf8");
const agentStateCss = readFileSync(agentStatePath, "utf8");
const repoRelative = (path: string) => relative(root, path).replaceAll("\\", "/");

const bespokeWorkingKeyframes = [
  "sidebar-hierarchy-breathe",
  "sidebar-hierarchy-drift",
  "ct-plan-sidebar-pulse",
  "pane-focus-pulse",
  "breathing-bar",
  "mem-pulse",
  "mem-pulse-card",
  "ct-plan-button-pulse",
  "ct-canvas-pulse",
  "ct-canvas-live",
  "ct-canvas-heartbeat-dot",
  "agent-icon-working-spin",
  "plan-companion-presence-pulse",
  "ct-spin",
];

for (const name of bespokeWorkingKeyframes) {
  assert.equal(
    globalsCss.includes(name),
    false,
    `src/styles/globals.css still contains bespoke working keyframe ${name}`,
  );
}

const ignoredDirectories = new Set(["node_modules", "dist", "target", "gen", "vendor"]);
function filesUnder(directory: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(directory)) {
    if (ignoredDirectories.has(entry)) continue;
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) files.push(...filesUnder(path));
    else files.push(path);
  }
  return files;
}

const shippedSourceExtensions = new Set([".css", ".js", ".jsx", ".rs", ".ts", ".tsx"]);
const srcFiles = filesUnder(srcRoot).filter((path) => shippedSourceExtensions.has(extname(path)));
const implementationFiles = srcFiles.filter((path) => !/\.test\.[^.]+$/.test(path));

function sourceOccurrences(pattern: RegExp): Array<{ path: string; matches: number }> {
  const occurrences: Array<{ path: string; matches: number }> = [];
  for (const path of implementationFiles) {
    const matches = readFileSync(path, "utf8").match(pattern)?.length ?? 0;
    if (matches > 0) occurrences.push({ path: repoRelative(path), matches });
  }
  return occurrences;
}

function componentOccurrences(pattern: RegExp): Array<{ path: string; matches: number }> {
  return sourceOccurrences(pattern).filter(({ path }) => /\.(?:ts|tsx)$/.test(path));
}

assert.deepEqual(
  componentOccurrences(/\b(?:function|const)\s+ActivityDots\b/g),
  [{ path: repoRelative(activityDotsPath), matches: 1 }],
  "ActivityDots must be the only working-indicator implementation under src",
);
assert.deepEqual(
  componentOccurrences(/\b(?:function|const)\s+LivingDots\b/g),
  [],
  "LivingDots must not exist under src",
);
assert.deepEqual(
  componentOccurrences(/\b(?:function|const)\s+SegmentedDonut\b/g),
  [{ path: repoRelative(segmentedDonutPath), matches: 1 }],
  "SegmentedDonut must be the only segmented-donut implementation under src",
);
assert.deepEqual(
  componentOccurrences(/conic-gradient\(/g),
  [{ path: repoRelative(segmentedDonutPath), matches: 1 }],
  "no second conic-gradient donut implementation may exist under src",
);

const tokenSheets = filesUnder(srcRoot)
  .filter((path) => path.endsWith("tokens.css"))
  .map(repoRelative);
assert.deepEqual(tokenSheets, ["src/styles/tokens.css"], "src must contain exactly one token sheet");

const phaseDotsRule = agentStateCss.match(/\.ct-phase-dots\s*\{([^}]*)\}/s);
assert(phaseDotsRule, ".ct-phase-dots rule must exist");
assert.doesNotMatch(
  phaseDotsRule[1],
  /--activity-dot-[\w-]+\s*:/,
  ".ct-phase-dots must not declare --activity-dot-*",
);

assert.equal(
  (agentStateCss.match(/@keyframes\s+ct-dots-squash-jump\b/g) ?? []).length,
  1,
  "the shared primitive must own exactly one squash keyframe",
);
assert.match(
  agentStateCss,
  /animation:\s*ct-dots-squash-jump[^;]*var\(--ct-ease-squash-jump\)/,
  "the shared squash keyframe must use --ct-ease-squash-jump",
);
const consumerSquashViolations: string[] = [];
for (const path of implementationFiles.filter((path) => path !== agentStatePath)) {
  const source = readFileSync(path, "utf8");
  if (/@keyframes\s+[\w-]*squash[\w-]*/i.test(source)) {
    consumerSquashViolations.push(`${repoRelative(path)} defines a consumer squash keyframe`);
  }
  if (/animation(?:-name)?\s*:[^;]*ct-dots-squash-jump/.test(source)) {
    consumerSquashViolations.push(`${repoRelative(path)} overrides the shared dot keyframe`);
  }
}

const sharedDotGeometrySelectors = new Set([
  ".activity-dots--micro",
  ".activity-dots--small",
  ".activity-dots--medium",
]);
const consumerDotGeometryViolations: string[] = [];
for (const path of filesUnder(stylesRoot).filter((candidate) => candidate.endsWith(".css"))) {
  const source = readFileSync(path, "utf8");
  for (const rule of source.matchAll(/([^{}]+)\{([^{}]*)\}/gs)) {
    const selector = rule[1].trim();
    for (const declaration of rule[2].matchAll(/--activity-dot-(size|gap|jump)\s*:/g)) {
      if (path === agentStatePath && sharedDotGeometrySelectors.has(selector)) continue;
      const offset = rule.index + rule[0].indexOf(declaration[0]);
      const line = source.slice(0, offset).split("\n").length;
      consumerDotGeometryViolations.push(
        `${repoRelative(path)}:${line} ${selector} declares --activity-dot-${declaration[1]}`,
      );
    }
  }
}
assert.deepEqual(
  consumerDotGeometryViolations,
  [],
  [
    "only shared ActivityDots size variants may declare dot geometry",
    "regression case: src/styles/globals.css:1924",
    ...consumerDotGeometryViolations,
  ].join("\n"),
);

const sweepName = "sidebar-plan-entry-completion-sweep";
assert.equal(
  (globalsCss.match(new RegExp(`@keyframes\\s+${sweepName}\\b`, "g")) ?? []).length,
  1,
  "completion sweep keyframes must exist exactly once",
);
assert.match(
  globalsCss,
  /\.sidebar-plan-entry-completion::after\s*\{[^}]*animation:\s*sidebar-plan-entry-completion-sweep\b/s,
  "completion sweep must be scoped to the plan entry card",
);
assert.equal(
  (globalsCss.match(new RegExp(sweepName, "g")) ?? []).length,
  2,
  "completion sweep must have only its plan-entry use and keyframe definition",
);

const activityDotsSource = readFileSync(activityDotsPath, "utf8");
assert.equal(
  (activityDotsSource.match(/size !== "micro" && <i \/>/g) ?? []).length,
  2,
  "micro must render one dot while small and medium render three",
);

const importPattern = /(?:from\s*|import\s*\(|require\s*\()\s*["'][^"']*playground(?:[/\\][^"']*)?["']/g;
const playgroundImports: string[] = [];
for (const sourceRoot of [join(root, "src"), join(root, "packages")]) {
  for (const path of filesUnder(sourceRoot).filter((candidate) => shippedSourceExtensions.has(extname(candidate)))) {
    if (importPattern.test(readFileSync(path, "utf8"))) playgroundImports.push(repoRelative(path));
    importPattern.lastIndex = 0;
  }
}
assert.deepEqual(playgroundImports, [], "src and packages must not import playground modules");

const repoLocalPlanDirectory = join(root, ".codeterm", "plans");
assert.equal(
  existsSync(repoLocalPlanDirectory),
  false,
  "repo-local plan artifact directory must not exist",
);

const centralizedPlanDatabase = resolve(homedir(), ".codeterm", "plans.db");
assert.equal(
  isInsideRepository(centralizedPlanDatabase),
  false,
  "centralized plan database must stay outside the repository",
);

const repoLocalPlanArtifacts = [
  repoLocalPlanDirectory,
  join(root, ".codeterm", "plans.db"),
  join(root, ".codeterm", "plan-index.json"),
].filter(existsSync).map(repoRelative);

const invariantViolations = [
  ...consumerSquashViolations,
  ...repoLocalPlanArtifacts.map((path) => `repo-local plan artifact exists: ${path}`),
];
assert.deepEqual(invariantViolations, [], invariantViolations.join("\n"));
console.log("motion integration source invariants: passed; ct-spin allowlist: none");
