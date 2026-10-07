import assert from "node:assert/strict";
import { discoverCodexModels } from "../src/models";
import { sessionExists } from "../src/sessions";

const reads: string[] = [];
(globalThis as any).host = {
  envGet: (key: string) => key === "CODEX_HOME" ? "/account" : null,
  homeDir: () => "/default",
  platform: () => "linux",
  log: () => {},
  fs: {
    fileExists: (path: string) => { reads.push(path); return true; },
    readJson: (path: string) => { reads.push(path); return { models: [{ slug: "fixture", visibility: "list" }] }; },
    readDir: (path: string) => { reads.push(path); return []; },
    readFile: (path: string) => { reads.push(path); return JSON.stringify({ id: "own", thread_name: "Fixture", updated_at: "2026-01-01T00:00:00Z" }); },
  },
};
assert.equal(discoverCodexModels()?.[0]?.id, "fixture");
assert.equal(sessionExists("/work", "own"), true);
assert.ok(reads.includes("/account/models_cache.json"));
assert.ok(reads.includes("/account/session_index.jsonl"));
assert.ok(reads.every((path) => path.startsWith("/account")));
console.log("account-home: passed");
