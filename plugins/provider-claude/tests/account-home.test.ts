import assert from "node:assert/strict";
import { effectiveReadPath } from "../../provider-shared/readHome";
import { claudeSessionFilePath } from "../src/session";
import { fetchClaudeUsage } from "../src/usage";

for (const [home, fallback, suffix, separator, expected] of [
  [null, "/default/projects", "projects", "/", "/default/projects"],
  ["", "/default/projects", "projects", "/", "/default/projects"],
  ["/account/", null, "projects", "/", "/account/projects"],
  ["C:\\account", null, "projects", "\\", "C:\\account\\projects"],
] as const) assert.equal(effectiveReadPath(home, fallback, suffix, separator), expected);

(globalThis as any).host = {
  envGet: (key: string) => key === "CLAUDE_CONFIG_DIR" ? "/account" : null,
  platform: () => "linux",
  expandHome: () => "/default/projects",
};
assert.equal(claudeSessionFilePath("/work", "own"), "/account/projects/-work/own.jsonl");

const reads: string[] = [];
let authorization: unknown;
const usage = fetchClaudeUsage({
  envGet: (key) => key === "CLAUDE_CONFIG_DIR" ? "/account" : null,
  platform: () => "macos",
  expandHome: () => "/default/.credentials.json",
  exec: () => { throw new Error("default account keychain must not be read"); },
  fs: { readFile: (path) => {
    reads.push(path);
    return JSON.stringify({ claudeAiOauth: { accessToken: "selected" } });
  } },
  fetch: (raw) => {
    authorization = JSON.parse(raw).headers.Authorization;
    return JSON.stringify({ status: 200, body: JSON.stringify({ five_hour: { utilization: 27 } }) });
  },
});
assert.deepEqual(reads, ["/account/.credentials.json"]);
assert.equal(authorization, "Bearer selected");
assert.equal(JSON.parse(usage).session_pct, 27);
console.log("account-home: passed");
