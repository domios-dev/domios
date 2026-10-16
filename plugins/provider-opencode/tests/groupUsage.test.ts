/**
 * Opencode per-group usage: store rows, balance, cache, and the yielded query.
 * Run: npx tsx plugins/provider-opencode/tests/groupUsage.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { ocHost } from "./hostFixture.testdata";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

function loadOpencodeWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

// Per-group usage is read from opencode's store and keyed by providerID.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { g: "deepseek", messages: 2, input: 100, output: 40, cache_read: 10, cache_write: 0, cost: 1.9425 },
        { g: null, messages: 9, input: 1, output: 1, cache_read: 0, cache_write: 0, cost: 0 },
      ],
    },
  }));
  eq(
    p.modelGroupUsage(),
    [{
      group: "deepseek",
      messages: 2,
      input_tokens: 100,
      output_tokens: 40,
      cache_read_tokens: 10,
      cache_write_tokens: 0,
      cost_micros: 1_942_500,
      spend_cents: 194,
    }],
    "modelGroupUsage maps rows to snake_case groups, adds observed spend, and drops a null provider",
  );
}

// A group with a live balance endpoint carries remaining credit; the limit is
// left absent because the endpoint exposes no included allowance.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  let fetched = "";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { g: "deepseek", messages: 1, input: 10, output: 5, cache_read: 0, cache_write: 0, cost: 3.39 },
        { g: "xai", messages: 1, input: 10, output: 5, cache_read: 0, cache_write: 0, cost: 118.75 },
      ],
    },
    fetch: (opts: string) => {
      fetched = opts;
      return JSON.stringify({
        status: 200,
        body: JSON.stringify({
          is_available: true,
          balance_infos: [{ currency: "USD", total_balance: "0.62", topped_up_balance: "0.62" }],
        }),
      });
    },
  }));
  const rows = p.modelGroupUsage() as Array<Record<string, unknown>>;
  const deepseek = rows.find((r) => r.group === "deepseek");
  const xai = rows.find((r) => r.group === "xai");
  assert(
    fetched.indexOf("api.deepseek.com") !== -1 &&
      fetched.indexOf("opencodeAuth") !== -1 &&
      fetched.indexOf("deepseekKey") !== -1,
    "the balance read names its own URL and the declared credential secret",
  );
  eq(deepseek?.spend_remaining_cents, 62, "deepseek remaining credit is read live");
  eq(deepseek?.spend_limit_cents, undefined, "no included limit is invented");
  eq(xai?.spend_cents, 11875, "xai spend comes from its own observed cost");
  eq(xai?.spend_remaining_cents, undefined, "a group without a balance endpoint has no remaining");
  eq(xai?.spend_limit_cents, undefined, "a group without a live limit reports null, never a guess");
}

// A failed balance read still reports the observed spend, with no fabricated limit.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const p = loadOpencodeWithHost(ocHost({
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      sqliteQuery: () => [
        { g: "deepseek", messages: 1, input: 10, output: 5, cache_read: 0, cache_write: 0, cost: 1.0 },
      ],
    },
    fetch: () => JSON.stringify({ error: "request failed: timed out" }),
  }));
  const rows = p.modelGroupUsage() as Array<Record<string, unknown>>;
  eq(rows[0].spend_cents, 100, "observed spend survives a failed balance read");
  eq(rows[0].spend_remaining_cents, undefined, "an unreadable balance leaves remaining absent");
}

{
  const p = loadOpencodeWithHost(ocHost({ fs: { fileExists: () => false } }));
  eq(p.modelGroupUsage(), [], "no session store ⇒ no usage, never a guess");
}

// Usage is re-read only when the db or its WAL changes, or after the TTL.
{
  const dbPath = "/data/opencode/opencode.db";
  let now = 1_000_000;
  let queries = 0;
  let wal = { size: 10, modifiedMs: 5 };
  let listed = true;
  const p = loadOpencodeWithHost(ocHost({
    envGet: (key: string) => key === "OPENCODE_DB" ? dbPath : null,
    unixNowMs: () => now,
    fs: {
      fileExists: (pth: string) => pth === dbPath,
      readDir: (dir: string) => !listed || dir !== "/data/opencode" ? [] : [
        { name: "opencode.db", size: 4096, modifiedMs: 1 },
        { name: "opencode.db-wal", size: wal.size, modifiedMs: wal.modifiedMs },
      ],
      sqliteQuery: () => {
        queries++;
        return [{ g: "xai", messages: queries, input: 0, output: 0, cache_read: 0, cache_write: 0, cost: 0 }];
      },
    },
  }));
  const messages = () => (p.modelGroupUsage() as Array<{ messages: number }>)[0].messages;
  eq(messages(), 1, "first usage read queries the store");
  now += 59_000;
  eq([messages(), queries], [1, 1], "an unchanged store inside the TTL is served from the cache");
  wal = { size: 20, modifiedMs: 5 };
  eq([messages(), queries], [2, 2], "a WAL size change re-reads the store");
  wal = { size: 20, modifiedMs: 6 };
  eq([messages(), queries], [3, 3], "a WAL mtime change re-reads the store");
  now += 60_000;
  eq([messages(), queries], [4, 4], "an expired TTL re-reads an unchanged store");
  listed = false;
  messages();
  messages();
  eq(queries, 6, "an unstamped store is never cached");
}

// Both heavy reads return the awaited query instead of holding the VM.
{
  const dbPath = "~/.local/share/opencode/opencode.db";
  const pending: Array<(rows: unknown[]) => unknown> = [];
  const host = ocHost({ fs: { fileExists: (pth: string) => pth === dbPath } }) as Record<string, any>;
  host.fs.sqliteQueryAsync = (_db: string, _sql: string, _params: unknown[], then: (rows: unknown[]) => unknown) => {
    pending.push(then);
    return { awaiting: pending.length };
  };
  const p = loadOpencodeWithHost(host);
  eq(p.modelGroupUsage(), { awaiting: 1 }, "modelGroupUsage yields on the usage query");
  eq(
    pending[0]([{ g: "xai", messages: 1, input: 2, output: 3, cache_read: 0, cache_write: 0, cost: 0 }]),
    [{ group: "xai", messages: 1, input_tokens: 2, output_tokens: 3, cache_read_tokens: 0, cache_write_tokens: 0, cost_micros: 0 }],
    "the usage continuation builds the result from the awaited rows",
  );
  eq(p.readStructuredChat("/work", "ses_owned", null), { awaiting: 2 }, "readStructuredChat yields on the part query");
  eq(pending[1]([]), { messages: [], observations: [], cursor: "[0,[]]" }, "the chat continuation keeps the cursor contract");
}

console.log(`
${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
