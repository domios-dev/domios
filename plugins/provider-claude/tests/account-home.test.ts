import { ioTestExport } from "../../provider-shared/tests/hostIo";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { effectiveReadPath } from "../../provider-shared/readHome";
import { claudeSessionFilePath } from "../src/session";
import { fetchClaudeUsage as ioSource_fetchClaudeUsage } from "../src/usage";
const fetchClaudeUsage = ioTestExport(ioSource_fetchClaudeUsage);

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

const accounts = [
  { home: null, service: "Claude Code-credentials", token: "usual", percent: 17 },
  { home: "/accounts/work", service: "Claude Code-credentials-b7e9950c", token: "work", percent: 73 },
  { home: "/accounts/missing", service: "Claude Code-credentials-98e96532", token: null, percent: null },
];
const byService = new Map(accounts.map(account => [account.service, account]));
for (const { home, service, token, percent } of accounts) {
  const fixture = {
    envGet: (name: string) => name === "CLAUDE_CONFIG_DIR" ? home : name === "USER" ? "fixture-owner" : null,
    platform: () => "macos",
    sha256: (text: string) => createHash("sha256").update(text).digest("hex"),
    expandHome: () => "/usual/.credentials.json",
    exec: (raw: string) => {
      const request = JSON.parse(raw);
      assert.equal(request.bin, "security");
      assert.deepEqual(request.args, ["find-generic-password", "-a", "fixture-owner", "-w", "-s", service]);
      const credential = byService.get(request.args.at(-1));
      return JSON.stringify(credential?.token
        ? { code: 0, stdout: JSON.stringify({ claudeAiOauth: { accessToken: credential.token } }) }
        : { code: 44 });
    },
    fs: { readFile: (path: string) => {
      assert.equal(path, `${home}/.credentials.json`);
      return null;
    } },
    fetch: (raw: string) => {
      assert.ok(token);
      assert.equal(JSON.parse(raw).headers.Authorization, `Bearer ${token}`);
      return JSON.stringify({ status: 200, body: JSON.stringify({ five_hour: { utilization: percent } }) });
    },
  };
  if (percent === null) assert.throws(() => fetchClaudeUsage(fixture), /usage credentials unavailable/);
  else assert.equal(JSON.parse(fetchClaudeUsage(fixture)).session_pct, percent);
}
console.log("account-home: passed");
