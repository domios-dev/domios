import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

export interface ContractResult {
  id: string;
  pass: boolean;
  detail: string;
}

export interface ContractSources {
  spec: string;
  fixture: string;
  baseFixtures: string;
}

// Topology-creation endpoints/verbs banned inside a @perf body regardless of quote
// style; only the measured pane-send path (/panes/<id>/send) may mutate (C7).
export const TOPOLOGY_ENDPOINTS = ["/agents/spawn", "/workspaces", "/tabs"];
export const TOPOLOGY_VERBS = ["spawnStub(", "injectCast(", "inject-cast", "preparePerfSeed(", "createPane(", "createWorkspace(", "awaitPtyReady("];

// Retained for the standalone body-token test; endpoint entries are quote-tagged.
export const TOPOLOGY_MUTATION_TOKENS = [
  ...TOPOLOGY_VERBS,
  ...TOPOLOGY_ENDPOINTS.flatMap((e) => [`"${e}`, `'${e}`, `\`${e}`]),
];

function normalizeQuotes(text: string): string {
  return text.replace(/['"`]/g, '"');
}

function topologyOffenders(body: string): string[] {
  const out: string[] = [];
  for (const verb of TOPOLOGY_VERBS) if (body.includes(verb)) out.push(verb);
  const norm = normalizeQuotes(body);
  for (const endpoint of TOPOLOGY_ENDPOINTS) {
    // Match the endpoint as a quoted path segment start, independent of quote char.
    if (new RegExp(`"${endpoint}(?:["/])`).test(norm)) out.push(endpoint);
  }
  // Pane creation via POST /panes (but NOT the measured /panes/<id>/send).
  if (/"\/panes"/.test(norm) || /post\(\s*"\/panes["/](?![^)]*\/send)/.test(norm)) out.push("/panes create");
  return out;
}

function perfTestBodies(specText: string): string[] {
  return [...specText.matchAll(/test\(\s*"@perf[\s\S]*?=>\s*\{/g)].map((m) => {
    const open = specText.indexOf("{", m.index! + m[0].length - 1);
    let depth = 0;
    for (let i = open; i < specText.length; i += 1) {
      if (specText[i] === "{") depth += 1;
      else if (specText[i] === "}" && --depth === 0) return specText.slice(open + 1, i);
    }
    return "";
  });
}

function perfTestParams(specText: string): string[] {
  return [...specText.matchAll(/test\(\s*"@perf[^"]*",\s*async\s*\(\s*\{([^}]*)\}/g)].map((m) => m[1]);
}

function fixtureBlock(fixtureText: string): string {
  const at = fixtureText.indexOf("perfSeed:");
  if (at < 0) return "";
  const open = fixtureText.indexOf("{", at);
  let depth = 0;
  for (let i = open; i < fixtureText.length; i += 1) {
    if (fixtureText[i] === "{") depth += 1;
    else if (fixtureText[i] === "}" && --depth === 0) return fixtureText.slice(at, i + 1);
  }
  return fixtureText.slice(at);
}

// Accepts ONLY: exactly one apiReset before prepare, zero in (prepare, use), optional
// teardown reset strictly after use. Any other apiReset layout fails.
export function seedLifetimeOrdered(fixtureText: string): boolean {
  const block = fixtureBlock(fixtureText);
  const prepareAt = block.indexOf("preparePerfSeed(");
  const useAt = block.indexOf("await use(");
  if (prepareAt < 0 || useAt < 0 || prepareAt >= useAt) return false;
  const resets = [...block.matchAll(/apiReset\s*\(/g)].map((m) => m.index!).sort((a, b) => a - b);
  const preSeed = resets.filter((o) => o < prepareAt).length;
  const between = resets.filter((o) => o > prepareAt && o < useAt).length;
  const teardown = resets.filter((o) => o > useAt).length;
  return preSeed === 1 && between === 0 && preSeed + teardown === resets.length && teardown <= 1;
}

// Pure checks over source text so they run against both the pre-fix and fixed trees.
export function runContract(sources: ContractSources, importers: string[]): ContractResult[] {
  const { spec, fixture, baseFixtures } = sources;
  const results: ContractResult[] = [];
  const push = (id: string, pass: boolean, detail: string) => results.push({ id, pass, detail });

  push("C1", !/\bensure[A-Z]\w*\(/.test(spec), "no ensure* calls under 00-perf-gate");

  const hasWorkerScope = /scope:\s*"worker"/.test(fixture);
  const hasFixtureTimeout = /timeout:\s*(DEV_FIXTURE_TIMEOUT_MS|\d[\d_]*)/.test(fixture);
  push("C2", hasWorkerScope && hasFixtureTimeout, 'perf fixture declares scope:"worker" and a fixture timeout');

  const fixtureArgs = [...fixture.matchAll(/(?:perfSeed|perfGate):\s*(?:\[\s*)?async\s*\(\s*\{([^}]*)\}/g)]
    .map((m) => m[1])
    .join(",");
  const forbiddenDeps = ["ui", "api", "paneCleanup"].filter((dep) => new RegExp(`\\b${dep}\\b`).test(fixtureArgs));
  push("C3", fixtureArgs !== "" && forbiddenDeps.length === 0, `perf fixture has no ui/api/paneCleanup dep (found: ${forbiddenDeps.join(",") || "none"})`);

  push("C4", !/test\.setTimeout\(/.test(spec) && !/test\.slow\(/.test(spec), "no test.setTimeout( or test.slow( under 00-perf-gate");

  push("C5", !/new Map\(/.test(spec) && !/let\s+\w*[Cc]ache\b/.test(spec), "no per-test cache constructs (new Map / *Cache)");

  const params = perfTestParams(spec);
  const paramsHaveApi = params.some((p) => /\bapi\b/.test(p));
  const block = fixtureBlock(fixture);
  const ownsClient = /new ApiClient\(/.test(block) && /preparePerfSeed\(/.test(block);
  const seedExposesApi = /\bapi\b/.test(fixture) && /await use\(/.test(fixture);
  const ordered = seedLifetimeOrdered(fixture);
  push(
    "C6",
    params.length === 4 && !paramsHaveApi && ownsClient && seedExposesApi && ordered,
    `4 @perf bodies take no shared api; fixture owns ApiClient, order preReset<prepare<use with no reset between (ordered: ${ordered})`,
  );

  const seedImportedFromSuites = importers.some((f) => f.replace(/\\/g, "/").includes("/suites/"));
  const onlyFixtureImports = importers.length === 1 && importers[0].replace(/\\/g, "/").endsWith("perf-fixtures.ts");
  push("SEED-IMPORT", onlyFixtureImports && !seedImportedFromSuites, `perf-seed imported only by perf-fixtures.ts, never from suites/ (importers: ${importers.join(",") || "none"})`);

  const bodies = perfTestBodies(spec);
  const c7Offenders = [...new Set(bodies.flatMap((b) => topologyOffenders(b)))];
  push("C7", bodies.length === 4 && c7Offenders.length === 0, `@perf bodies mutate no topology (offenders: ${c7Offenders.join(",") || "none"})`);

  const resetSites = [...baseFixtures.matchAll(/apiReset\s*\(/g)];
  const apiBlockStart = baseFixtures.indexOf("api: [");
  const apiBlockRange = (() => {
    if (apiBlockStart < 0) return null;
    const arrow = baseFixtures.indexOf("=>", apiBlockStart);
    const open = baseFixtures.indexOf("{", arrow);
    let depth = 0;
    for (let i = open; i < baseFixtures.length; i += 1) {
      if (baseFixtures[i] === "{") depth += 1;
      else if (baseFixtures[i] === "}" && --depth === 0) return { start: open, end: i };
    }
    return null;
  })();
  const allResetsInApiFixture =
    apiBlockRange !== null && resetSites.every((m) => m.index! >= apiBlockRange.start && m.index! <= apiBlockRange.end);
  const autoFixtureResets = (() => {
    for (const m of baseFixtures.matchAll(/auto:\s*true/g)) {
      const around = baseFixtures.slice(Math.max(0, m.index! - 600), m.index! + 600);
      if (/apiReset\s*\(/.test(around)) return true;
    }
    return false;
  })();
  push(
    "C8",
    resetSites.length > 0 && allResetsInApiFixture && !autoFixtureResets,
    `apiReset only inside the non-auto api fixture; no auto:true fixture resets (sites: ${resetSites.length}, auto-reset: ${autoFixtureResets})`,
  );

  return results;
}

function walkTsFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === ".git") continue;
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) walkTsFiles(full, out);
    else if (/\.(ts|tsx)$/.test(entry)) out.push(full);
  }
  return out;
}

// Pure fs traversal (no shell/grep, Windows-safe): importers of perf-seed via import.
export function seedImporters(rootDir: string): string[] {
  const importRe = /import[\s\S]*?from\s*["'][^"']*\/perf-seed["']/;
  return walkTsFiles(rootDir).filter((file) => !file.endsWith(".test.ts") && importRe.test(readFileSync(file, "utf8")));
}
