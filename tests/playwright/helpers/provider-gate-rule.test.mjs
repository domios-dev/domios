/**
 * WHAT: the provider-source rule fires on an ungated agent-spawning test.
 * WHY: a label with no checker is decoration; the rule only earns trust if the
 *      defective input is proven to FAIL, not just the fixed input to pass.
 * ORACLE: findings returned for synthetic specs this test supplies.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { PROVIDER_GATE_REASON, providerGateFindings } from "./provider-gate-rule.mjs";

function findings(source) {
  return providerGateFindings(source, "synthetic.spec.ts");
}

const UNGATED = `
test("@golden pasting a screenshot attaches it", async ({ spawnAgent }) => {
  const spawned = await spawnAgent({ agent: "claude", model: "sonnet" });
  expect(spawned.tab_id).toBeTruthy();
});
`;

const STUB_TAGGED = `
import { isStubTransportActive } from "../../helpers/stub-gate";
test("@stub pasting a screenshot attaches it", async ({ api, spawnAgent }) => {
  test.skip(!(await isStubTransportActive(api)), "no stub transport");
  const spawned = await spawnAgent({ agent: "claude", model: "sonnet" });
  expect(spawned.tab_id).toBeTruthy();
});
`;

// ── negative control: the defect must be caught ──

test("an ungated agent-spawning test is a violation", () => {
  const found = findings(UNGATED);
  assert.equal(found.length, 1, "expected exactly 1 finding for the ungated spec");
  assert.equal(found[0].reason, PROVIDER_GATE_REASON);
  assert.deepEqual(found[0].declarations, []);
  assert.equal(found[0].line, 2);
});

test("the same test passes once it is routed through the stub", () => {
  assert.deepEqual(findings(STUB_TAGGED), []);
});

test("@live alone satisfies the rule", () => {
  assert.deepEqual(
    findings(`test("@live real spawn", async ({ spawnAgent }) => { await spawnAgent({ agent: "claude" }); });`),
    [],
  );
});

test("a bare stub gate with no tag satisfies the rule", () => {
  assert.deepEqual(
    findings(`
test("@golden gated", async ({ api, spawnAgent }) => {
  test.skip(!(await isStubTransportActive(api)), "no stub transport");
  await spawnAgent({ agent: "claude" });
});
`),
    [],
  );
});

test("a beforeEach gate in the enclosing describe covers its tests", () => {
  assert.deepEqual(
    findings(`
test.describe("agent flows on the stub transport", () => {
  test.beforeEach(async ({ api }) => {
    test.skip(!(await isStubTransportActive(api)), "no stub transport");
  });
  test("@golden claude: spawn works", async ({ spawnAgent }) => {
    await spawnAgent({ agent: "claude" });
  });
});
`),
    [],
  );
});

test("a beforeEach gate in a sibling describe does NOT cover an outer test", () => {
  const found = findings(`
test.describe("gated", () => {
  test.beforeEach(async ({ api }) => {
    test.skip(!(await isStubTransportActive(api)), "no stub transport");
  });
});
test("@golden ungated sibling", async ({ spawnAgent }) => {
  await spawnAgent({ agent: "claude" });
});
`);
  assert.equal(found.length, 1);
  assert.deepEqual(found[0].declarations, []);
});

// ── "exactly one", not "at least one" ──

test("carrying both @stub and @live is a violation", () => {
  const found = findings(
    `test("@stub @live both", async ({ spawnAgent }) => { await spawnAgent({ agent: "claude" }); });`,
  );
  assert.equal(found.length, 1);
  assert.deepEqual(found[0].declarations, ["stub", "live"]);
});

test("a @live test that also carries a stub gate is a violation", () => {
  const found = findings(`
test("@live contradictory", async ({ api, spawnAgent }) => {
  test.skip(!(await isStubTransportActive(api)), "no stub transport");
  await spawnAgent({ agent: "claude" });
});
`);
  assert.equal(found.length, 1);
  assert.deepEqual(found[0].declarations, ["stub", "live"]);
});

// ── the rule only applies to spawns that can reach a real CLI ──

test("a spawn naming a bogus provider is not an agent spawn", () => {
  assert.deepEqual(
    findings(`
test("@smoke spawn with an unknown provider errors honestly", async ({ api }) => {
  await api.post("/agents/spawn", { agent: "definitely-no-such-provider" });
});
`),
    [],
  );
});

test("a REST spawn with a computed provider id is an agent spawn", () => {
  const found = findings(`
test("@golden rest spawn", async ({ api }) => {
  await api.post("/agents/spawn", { agent: restProvider.id, activate: true });
});
`);
  assert.equal(found.length, 1);
});

test("a test that never spawns is untouched by the rule", () => {
  assert.deepEqual(
    findings(`test("@golden no spawn here", async ({ ui }) => { await ui.page.click("#x"); });`),
    [],
  );
});

test("template-literal titles still expose their tag", () => {
  assert.deepEqual(
    findings("test(`@stub ${provider}: spawn works`, async ({ spawnAgent }) => { await spawnAgent({ agent: provider }); });"),
    [],
  );
});

test("test.fixme is out of scope, like the rest of the checker", () => {
  assert.deepEqual(
    findings(`test.fixme("@golden watcher tab", async ({ spawnAgent }) => { await spawnAgent({ agent: "claude" }); });`),
    [],
  );
});
