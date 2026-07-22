import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  hardMetricSkipViolation,
  runContract,
  seedActiveLogicSound,
  seedImporters,
  seedLifetimeOrdered,
  skipIsEnvOnly,
  specHasEnvPresentThrowGuard,
  TOPOLOGY_MUTATION_TOKENS,
  type ContractSources,
} from "./perf-gate-contract";

const specPath = resolve(__dirname, "..", "suites", "00-perf-gate", "perf-gate.spec.ts");
const fixturePath = resolve(__dirname, "perf-fixtures.ts");
const seedPath = resolve(__dirname, "perf-seed.ts");
const baseFixturesPath = resolve(__dirname, "fixtures.ts");

const sources: ContractSources = {
  spec: readFileSync(specPath, "utf8"),
  fixture: readFileSync(fixturePath, "utf8"),
  baseFixtures: readFileSync(baseFixturesPath, "utf8"),
  seed: readFileSync(seedPath, "utf8"),
};
const realFixture = sources.fixture;
const realSeed = sources.seed;
const results = runContract(sources, seedImporters(resolve(__dirname, "..")));
const byId = (id: string) => results.find((r) => r.id === id)!;

for (const id of ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10", "SEED-IMPORT"]) {
  test(`${id}: ${byId(id).detail}`, () => {
    assert.ok(byId(id).pass, byId(id).detail);
  });
}

function c7Passes(spec: string): boolean {
  return runContract({ ...sources, spec }, seedImporters(resolve(__dirname, ".."))).find((r) => r.id === "C7")!.pass;
}

function c6Passes(fixture: string): boolean {
  return runContract({ ...sources, fixture }, seedImporters(resolve(__dirname, ".."))).find((r) => r.id === "C6")!.pass;
}

test("C6 negative: post-seed/pre-use reset fails", () => {
  const mutated = realFixture.replace("const seed = await preparePerfSeed(api);", "const seed = await preparePerfSeed(api);\n      await apiReset(api);");
  assert.ok(seedLifetimeOrdered(realFixture), "real fixture ordering must pass");
  assert.equal(seedLifetimeOrdered(mutated), false, "reset between prepare and use must fail");
  assert.equal(c6Passes(mutated), false, "C6 must fail on post-seed/pre-use reset");
});

test("C6 negative: two pre-seed resets fails", () => {
  const mutated = realFixture.replace(
    /([ \t]*)await apiReset\(api\);\r?\n([ \t]*)const seed/,
    "$1await apiReset(api);\n$2await apiReset(api);\n$2const seed",
  );
  assert.notEqual(mutated, realFixture, "mutation must add a second pre-seed reset");
  assert.equal(seedLifetimeOrdered(mutated), false, "two pre-seed resets must fail");
  assert.equal(c6Passes(mutated), false, "C6 must fail on two pre-seed resets");
});

test("C6 negative: zero pre-seed reset fails", () => {
  const mutated = realFixture.replace(/[ \t]*await apiReset\(api\);\r?\n(?=[ \t]*const seed)/, "");
  assert.notEqual(mutated, realFixture, "mutation must remove the pre-seed reset");
  assert.equal(seedLifetimeOrdered(mutated), false, "zero pre-seed reset must fail");
});

const realBody = 'const { api } = perfSeed;\n  await focusTerminal(page, api, perfSeed.write.front);\n  await api.post(`/panes/${perfSeed.write.front}/send`, { text: "x" });';
function specWithBody(body: string): string {
  return `test("@perf x", async ({ ui, perfSeed }) => {\n  ${body}\n  expect(1).toBe(1);\n});\ntest("@perf y", async ({ ui, perfSeed }) => { expect(1).toBe(1); });\ntest("@perf z", async ({ ui, perfSeed }) => { expect(1).toBe(1); });\ntest("@perf w", async ({ ui, perfSeed }) => { expect(1).toBe(1); });`;
}

test("C7 positive: real bodies (only measured pane send) pass", () => {
  assert.ok(byId("C7").pass, "real @perf bodies must pass C7");
  assert.ok(c7Passes(specWithBody(realBody)), "measured /panes/<id>/send must be allowed");
});

test("C7 negative: backtick-quoted topology create fails", () => {
  assert.equal(c7Passes(specWithBody("await api.post(`/agents/spawn`, {});")), false, "backtick /agents/spawn must fail");
});

test("C7 negative: single-quoted topology create fails", () => {
  assert.equal(c7Passes(specWithBody("await api.post('/workspaces', {});")), false, "single-quoted /workspaces must fail");
});

test("C7 negative: /tabs pane-create fails under any quote", () => {
  assert.equal(c7Passes(specWithBody("await api.post(`/workspaces/${wsId}/tabs`, {});")), false, "/tabs pane-create must fail");
  assert.equal(c7Passes(specWithBody("await api.post('/tabs', {});")), false, "single-quoted /tabs must fail");
});

test("seedImporters: fs traversal handles paths with spaces, no shell", () => {
  const root = mkdtempSync(join(tmpdir(), "perf seed importers test-"));
  try {
    const nested = join(root, "nested dir", "deep space");
    mkdirSync(nested, { recursive: true });
    const importer = join(nested, "importer.ts");
    const nonImporter = join(nested, "other.ts");
    const testFile = join(nested, "analyzer.test.ts");
    writeFileSync(importer, 'import { preparePerfSeed } from "../../helpers/perf-seed";\n');
    writeFileSync(nonImporter, 'import { foo } from "./bar";\n');
    writeFileSync(testFile, 'import { x } from "../../helpers/perf-seed";\n');
    const found = seedImporters(root).map((p) => p.replace(/\\/g, "/"));
    assert.equal(found.length, 1, `exactly one importer expected, got ${found.join(",")}`);
    assert.ok(found[0].endsWith("importer.ts"), "the spaces-path importer must be detected");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("SEED-IMPORT negative: a suites/ importer is flagged as a violation", () => {
  const root = mkdtempSync(join(tmpdir(), "perf-seed-suites-"));
  try {
    const helpers = join(root, "helpers");
    const suites = join(root, "suites", "00-perf-gate");
    mkdirSync(helpers, { recursive: true });
    mkdirSync(suites, { recursive: true });
    writeFileSync(join(helpers, "perf-fixtures.ts"), 'import { preparePerfSeed } from "./perf-seed";\n');
    writeFileSync(join(suites, "perf-gate.spec.ts"), 'import { preparePerfSeed } from "../../helpers/perf-seed";\n');
    const importers = seedImporters(root);
    const result = runContract(sources, importers).find((r) => r.id === "SEED-IMPORT")!;
    assert.equal(result.pass, false, "a suites/ importer must make SEED-IMPORT fail");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

function c9Passes(over: Partial<ContractSources>): boolean {
  return runContract({ ...sources, ...over }, seedImporters(resolve(__dirname, ".."))).find((r) => r.id === "C9")!.pass;
}

test("C9 positive: real spec+seed gate on env only, bypass probe, cache true-only", () => {
  assert.ok(byId("C9").pass, byId("C9").detail);
  assert.ok(skipIsEnvOnly(sources.spec), "skip must gate on env-derived perfSeed.active");
  assert.ok(seedActiveLogicSound(realSeed), "seed must bypass the probe and cache true-only");
});

test("C9 negative: skip depending on a probe result is RED", () => {
  const probeSkip = sources.spec.replace(
    "test.skip(!perfSeed.active, STUB_ENV_ABSENT_SKIP);",
    'test.skip(!(await isStubTransportActive(api)), "probe");',
  );
  assert.ok(probeSkip !== sources.spec, "mutation must apply");
  assert.equal(skipIsEnvOnly(probeSkip), false, "probe-based skip must not be env-only");
  assert.equal(c9Passes({ spec: probeSkip }), false, "C9 must fail when skip depends on a probe");
});

test("C9 negative: seed re-introducing the memoized probe is RED", () => {
  const probeSeed = realSeed.replace("const active = cachedActiveTrue || stubEnvActive();", "const active = await isStubTransportActive(api);");
  assert.equal(seedActiveLogicSound(probeSeed), false, "probe-based active must be unsound");
  assert.equal(c9Passes({ seed: probeSeed }), false, "C9 must fail when seed uses the memoized probe");
});

test("C9 negative: seed caching false is RED", () => {
  const cacheFalseSeed = realSeed.replace("cachedActiveTrue = true;", "cachedActiveTrue = true;\n  if (!active) cachedActiveTrue = false;");
  assert.equal(seedActiveLogicSound(cacheFalseSeed), false, "caching false must be unsound");
});

test("C9 negative: seed swallowing a seeding failure into active=false is RED", () => {
  const swallowSeed = realSeed.replace(
    "const writeFront = await spawnStub(api, \"pw-perf-write-render\");",
    "const writeFront = await spawnStub(api, \"pw-perf-write-render\").catch(() => { active = false; return \"\"; });",
  );
  assert.equal(seedActiveLogicSound(swallowSeed), false, "a .catch swallowing seeding into active=false must be unsound");
  assert.equal(c9Passes({ seed: swallowSeed }), false, "C9 must fail on swallow-to-false");
});

test("release-gate guard: skip-under-env-present is a violation, env-absent is allowed", () => {
  assert.equal(hardMetricSkipViolation(true, ["pty-write-to-render"]), true, "hard-metric skip under env-present must violate");
  assert.equal(hardMetricSkipViolation(true, ["some-soft-metric"]), false, "non-hard-metric skip is not a hard violation");
  assert.equal(hardMetricSkipViolation(false, ["pty-write-to-render"]), false, "env-absent skip is allowed");
});

function c10Passes(over: Partial<ContractSources>): boolean {
  return runContract({ ...sources, ...over }, seedImporters(resolve(__dirname, ".."))).find((r) => r.id === "C10")!.pass;
}

test("C10 positive: spec throws on env-present+!active before test.skip", () => {
  assert.ok(byId("C10").pass, byId("C10").detail);
  assert.ok(specHasEnvPresentThrowGuard(sources.spec), "spec must fail loud before skip");
});

test("C10 negative: spec missing the throw-guard (skip reachable under env-present) is RED", () => {
  const noGuard = sources.spec.replace(/^.*if \(stubEnvActive\(\) && !perfSeed\.active\) throw [^\r\n]*\r?\n/m, "");
  assert.ok(noGuard !== sources.spec, "mutation must remove the guard");
  assert.equal(specHasEnvPresentThrowGuard(noGuard), false, "no guard => not fail-loud");
  assert.equal(c10Passes({ spec: noGuard }), false, "C10 must fail when the throw-guard is missing");
});

test("readiness/banner timeout THROWS (never active=false/skip)", async () => {
  const { awaitStubBanner } = await import("./perf-seed");
  const noBannerApi = { get: async () => "not-the-banner-yet" } as unknown as Parameters<typeof awaitStubBanner>[0];
  const start = Date.now();
  await assert.rejects(() => awaitStubBanner(noBannerApi, "p1", "pw-perf", 40), /stub banner/, "banner timeout must throw");
  assert.ok(Date.now() - start < 2_000, "the factored cap keeps the timeout path fast");
});

test("readiness guard is wired into spawn seeding (throw propagates through preparePerfSeed)", () => {
  assert.match(realSeed, /await awaitStubBanner\(api, spawned\.pane_id, title\)/, "spawnStub must await the banner helper");
  assert.doesNotMatch(realSeed, /awaitStubBanner[\s\S]{0,60}\.catch/, "banner wait must not be swallowed");
});

test("runtime: env-absent yields active=false with the explicit incomplete reason", async () => {
  const { preparePerfSeed, stubEnvActive, STUB_ENV_ABSENT_SKIP } = await import("./perf-seed");
  assert.equal(stubEnvActive({}), false, "no env => not active");
  assert.match(STUB_ENV_ABSENT_SKIP, /CODETERM_STUB_AGENT_CMD/, "skip reason must name the env var");
  const prev = process.env.CODETERM_STUB_AGENT_CMD;
  delete process.env.CODETERM_STUB_AGENT_CMD;
  try {
    const throwingApi = { post: async () => { throw new Error("must not be called"); } } as unknown as Parameters<typeof preparePerfSeed>[0];
    const seed = await preparePerfSeed(throwingApi);
    assert.equal(seed.active, false, "env-absent must return active=false without spawning");
  } finally {
    if (prev === undefined) delete process.env.CODETERM_STUB_AGENT_CMD;
    else process.env.CODETERM_STUB_AGENT_CMD = prev;
  }
});

test("runtime: env-present + spawn failure THROWS, never active=false/skip", async () => {
  const { preparePerfSeed, stubEnvActive } = await import("./perf-seed");
  const prev = process.env.CODETERM_STUB_AGENT_CMD;
  process.env.CODETERM_STUB_AGENT_CMD = "node fixtures/stub-agent/replay.mjs";
  try {
    assert.equal(stubEnvActive(), true, "env present => active intent");
    const failingApi = { post: async () => { throw new Error("spawn boom"); } } as unknown as Parameters<typeof preparePerfSeed>[0];
    await assert.rejects(() => preparePerfSeed(failingApi), /spawn boom/, "seeding failure must throw, not skip");
  } finally {
    if (prev === undefined) delete process.env.CODETERM_STUB_AGENT_CMD;
    else process.env.CODETERM_STUB_AGENT_CMD = prev;
  }
});

test("test bodies contain no spawn/inject/readiness setup", () => {
  const bodies = [...sources.spec.matchAll(/test\(\s*"@perf[\s\S]*?=>\s*\{/g)].map((m) => {
    const open = sources.spec.indexOf("{", m.index! + m[0].length - 1);
    let depth = 0;
    for (let i = open; i < sources.spec.length; i += 1) {
      if (sources.spec[i] === "{") depth += 1;
      else if (sources.spec[i] === "}" && --depth === 0) return sources.spec.slice(open + 1, i);
    }
    return "";
  });
  assert.equal(bodies.length, 4, "expected 4 @perf test bodies");
  for (const body of bodies) {
    for (const token of TOPOLOGY_MUTATION_TOKENS) {
      assert.ok(!body.includes(token), `test body must not contain setup token ${token}`);
    }
  }
});

test("the seed module front-loads work before returning handles", () => {
  const seed = readFileSync(seedPath, "utf8");
  const spawnAt = seed.indexOf("spawnStub(");
  const returnAt = seed.lastIndexOf("return {");
  assert.ok(spawnAt >= 0 && returnAt >= 0 && spawnAt < returnAt, "seed spawns before returning prepared handles");
  assert.match(seed, /export async function preparePerfSeed/);
});
