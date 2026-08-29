import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(__dirname, "..", "..", "..");
const read = (relativePath: string) => readFileSync(resolve(root, relativePath), "utf8");

test("stub web-chat coverage cannot enter the full project under real-provider transport", () => {
  const config = read("tests/playwright/playwright.config.ts");
  const webSpec = read("tests/playwright/suites/12-web/web.spec.ts");

  assert.match(webSpec, /@stub web chat: an assistant reply streams in and history survives a reload/);
  assert.match(config, /name:\s*"stub",\s*grep:\s*\/@stub\//s);
  assert.match(config, /name:\s*"full",\s*grepInvert:\s*\/@quarantine\|@restart\|@live\|@perf\|@stub\//s);
});
