import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  runContract,
  seedImporters,
  seedLifetimeOrdered,
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
};
const realFixture = sources.fixture;
const results = runContract(sources, seedImporters(resolve(__dirname, "..")));
const byId = (id: string) => results.find((r) => r.id === id)!;

for (const id of ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "SEED-IMPORT"]) {
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
  const mutated = realFixture.replace("await apiReset(api);\n      const seed", "await apiReset(api);\n      await apiReset(api);\n      const seed");
  assert.equal(seedLifetimeOrdered(mutated), false, "two pre-seed resets must fail");
  assert.equal(c6Passes(mutated), false, "C6 must fail on two pre-seed resets");
});

test("C6 negative: zero pre-seed reset fails", () => {
  const mutated = realFixture.replace("      await apiReset(api);\n      const seed", "      const seed");
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
