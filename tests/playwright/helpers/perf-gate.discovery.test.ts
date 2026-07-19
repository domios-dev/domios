import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(__dirname, "..", "..", "..");
const read = (relativePath: string) => readFileSync(resolve(root, relativePath), "utf8");

test("the dedicated perf project discovers only @perf tests", () => {
  const config = read("tests/playwright/playwright.config.ts");
  const packageJson = JSON.parse(read("package.json")) as { scripts?: Record<string, string> };
  assert.equal((config.match(/name:\s*"perf"/g) ?? []).length, 1);
  assert.match(config, /name:\s*"perf",\s*grep:\s*\/@perf\//s);
  assert.match(config, /name:\s*"perf",[\s\S]*?timeout:\s*300_000/s);
  assert.equal(packageJson.scripts?.["test:pw:perf"], "playwright test -c tests/playwright --project=perf");
});

test("the perf gate names all four user-visible stub-provider measurements", () => {
  const spec = read("tests/playwright/suites/00-perf-gate/perf-gate.spec.ts");
  const seed = read("tests/playwright/helpers/perf-seed.ts");
  for (const metric of [
    "pty-write-to-render",
    "long-history-bottom-anchor",
    "cold-interactive",
    "ipc-starvation-visible-render",
  ]) {
    assert.match(spec, new RegExp(`name: "${metric}"`));
  }
  assert.match(seed, /stubEnvActive/);
  assert.doesNotMatch(seed, /isStubTransportActive/);
  assert.match(seed, /STUB_PROVIDER\s*=\s*"claude"/);
  assert.doesNotMatch(spec, /@live/);
});

test("the hidden-pane render budget emits a sample summary artifact", () => {
  const spec = read("tests/playwright/suites/12-web/multitab-perf.spec.ts");
  assert.match(spec, /const HIDDEN_PANE_RENDER_BUDGET = 3/);
  assert.match(spec, /name: "hidden-pane-render-delta"/);
  assert.match(spec, /samples: hiddenRenderDeltas/);
  assert.match(spec, /budget: HIDDEN_PANE_RENDER_BUDGET/);
  assert.match(spec, /toBeLessThanOrEqual\(HIDDEN_PANE_RENDER_BUDGET\)/);
});
