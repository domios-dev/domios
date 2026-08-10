import assert from "node:assert/strict";
import test from "node:test";
import { parseIsolatedPlaywrightArgs, playwrightArgs } from "../../../scripts/run-playwright-isolated";

const FILE = "tests/playwright/suites/01-smoke/smoke.spec.ts";

test("isolated runner requires one explicit spec, title, and project", () => {
  const parsed = parseIsolatedPlaywrightArgs(["--file", FILE, "--grep", "real UI", "--project", "smoke"]);
  assert.equal(parsed.grep, "real UI");
  assert.equal(parsed.project, "smoke");
  // Forward slashes, never a native separator: the value is a regex to Playwright.
  assert.equal(parsed.file, FILE);
  assert.ok(!parsed.file.includes("\\"));
  assert.deepEqual(playwrightArgs(parsed).slice(-7), [
    `--project=smoke`,
    parsed.file,
    "-g",
    "real UI",
    "--timeout=30000",
    "--global-timeout=600000",
    "--reporter=line",
  ]);
});

test("the documented pnpm form works: a forwarded leading -- is ignored", () => {
  const documented = ["--", "--file", FILE, "--grep", "real UI", "--project", "smoke"];
  assert.deepEqual(parseIsolatedPlaywrightArgs(documented), parseIsolatedPlaywrightArgs(documented.slice(1)));
});

test("only the leading separator is tolerated; a stray -- is still refused", () => {
  assert.throws(
    () => parseIsolatedPlaywrightArgs(["--file", FILE, "--", "--grep", "x", "--project", "smoke"]),
    /unsupported option/,
  );
});

test("isolated runner rejects broad and out-of-tree invocations", () => {
  assert.throws(() => parseIsolatedPlaywrightArgs(["--file", FILE, "--project", "smoke"]), /--grep/);
  assert.throws(
    () => parseIsolatedPlaywrightArgs(["--file", "package.json", "--grep", "x", "--project", "smoke"]),
    /under tests\/playwright/,
  );
  assert.throws(
    () => parseIsolatedPlaywrightArgs(["--file", FILE, "--grep", "x", "--project", "smoke", "--timeout", "1"]),
    /unsupported option/,
  );
});
