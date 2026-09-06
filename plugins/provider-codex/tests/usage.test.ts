/**
 * Codex usage-panel parser tests (parseCodexUsage / fetchCodexUsage).
 * Run: npx tsx plugins/provider-codex/tests/usage.test.ts
 *
 * Codex 0.153.0 restructured the `/status` screen: gauge bars are now
 * bracketed ("[████░░░░]") and moved from one flat list of "<Model> <field>
 * limit" lines into a two-level shape — an unqualified account-level
 * "Weekly limit:" followed by a "<Model> limit:" header whose own "5h
 * limit:"/"Weekly limit:" rows are nested two-plus lines below the header
 * that names them. The pre-existing single-line lookback lost the model
 * name by the time it reached the second nested row and misclassified a
 * per-model weekly reading as the general weekly figure. See usage.ts's
 * `stripCodexBorder` / `isCodexSectionHeader` / sticky `sectionHeader` for
 * the fix: a section header now stays in scope across every gauge row
 * nested under it, not just the one immediately below it.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseCodexUsage, fetchCodexUsage } from "../src/usage";

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

const here = path.dirname(fileURLToPath(import.meta.url));

function fixtureText(name: string): string {
  return fs.readFileSync(path.join(here, "fixtures", name), "utf8");
}

type Usage = {
  session_pct: number | null;
  session_resets_at_ms: number | null;
  weekly_pct: number | null;
  weekly_resets_at_ms: number | null;
} | null;

console.log("\n--- parseUsage: real Codex 0.153.0 /status capture ---\n");
{
  // Captured live from an installed `codex-cli 0.153.0` via `/status`
  // (plugins/provider-codex/tests/fixtures/codex-status-v0-153-0.txt).
  const now = new Date(2026, 8, 3, 12, 0, 0, 0).getTime();
  const usage = parseCodexUsage(fixtureText("codex-status-v0-153-0.txt"), now) as Usage;

  assert(usage !== null, "v0.153.0 capture parses to a snapshot, not unknown");
  eq(usage?.weekly_pct, 1, "v0.153.0: account-level 'Weekly limit: 99% left' -> 1% used");
  eq(
    usage?.weekly_resets_at_ms,
    new Date(2026, 8, 11, 9, 47, 0, 0).getTime(),
    "v0.153.0: weekly reset 'resets 09:47 on 11 Sep' parsed against the account-level row",
  );
  eq(
    usage?.session_pct,
    null,
    "v0.153.0: no unqualified 5h figure exists in this capture -> unknown, not the per-model 100%",
  );

  // The regression this fixture exists to catch: the model's OWN weekly
  // figure (55% left, nested under "GPT-5.3-Codex-Spark limit:") must never
  assert(usage?.weekly_pct !== 45, "v0.153.0: the per-model weekly (55% left -> 45 used) is excluded, not reported");
}

console.log("\n--- parseUsage: unknown vs zero are different states, never the same value ---\n");
{
  const now = new Date(2026, 5, 14, 12, 0, 0, 0).getTime();

  // Nothing recognizable at all: must be the unknown sentinel (null), never
  // an object coerced to 0. This is the exact regression this whole task
  const garbage = parseCodexUsage("OpenAI Codex\nloading…\nsomething else entirely", now) as Usage;
  assert(garbage === null, "fully unparsable screen returns null (unknown), not an object");

  // A real, legitimate 0%-used reading is a *different* representation:
  // a non-null object carrying the literal value 0 — proving the
  const zeroUsed = parseCodexUsage("General 5h limit █ 0% used (resets 19:59)", now) as Usage;
  assert(zeroUsed !== null, "a genuine 0%-used reading still returns a snapshot object");
  eq(zeroUsed?.session_pct, 0, "genuine 0%-used reading keeps the literal 0 value");

  assert(
    garbage === null && zeroUsed !== null && zeroUsed?.session_pct === 0,
    "unknown (null) and a real 0% reading (object with pct 0) are never conflated",
  );

  // A close-but-broken line (gauge shape present, no resets clause) must
  // still fall back to unknown rather than guessing a partial value.
  eq(parseCodexUsage("General 5h limit 10% left", now), null, "gauge without a resets clause -> unknown, not a guess");
}

console.log("\n--- parseUsage: structural match survives copy changes (not literal-label matching) ---\n");
{
  // Same nested shape as the real v0.153.0 capture (account weekly, then a
  // "<Model> limit:" header owning its own 5h + weekly rows two lines
  const altered = [
    "Account:  someone-else@example.org (Team)",
    "Session:  ffffffff-0000-1111-2222-333344445555",
    "",
    "Weekly limit:   [====================] 88% left (resets 14:00 on 2 Oct)",
    "Opus-4.9-Preview limit:",
    "5h limit:   [====================] 70% left (resets 05:10)",
    "Weekly limit:   [==========..........] 33% left (resets 08:00 on 1 Oct)",
  ].join("\n");
  const now = new Date(2026, 8, 20, 9, 0, 0, 0).getTime();
  const usage = parseCodexUsage(altered, now) as Usage;

  assert(usage !== null, "copy-altered nested fixture still parses");
  eq(usage?.weekly_pct, 12, "copy-altered: account-level '88% left' -> 12 used, picked over the model's own weekly");
  eq(usage?.session_pct, null, "copy-altered: only a per-model 5h figure exists -> unknown, not the model's 30 used");
}

console.log("\n--- parseUsage never manufactures a timeout: unknown carries no failure-kind label ---\n");
{
  // usage.ts owns exactly one thing here: turning /status text into a
  // snapshot or `null`. It never throws and never embeds a failure-kind
  const now = new Date(2026, 5, 14, 12, 0, 0, 0).getTime();
  let threw = false;
  let result: unknown;
  try {
    result = parseCodexUsage("not a status screen at all", now);
  } catch {
    threw = true;
  }
  assert(!threw, "parseCodexUsage never throws for unparsable input (nothing for a caller to mislabel)");
  assert(result === null, "parseCodexUsage's failure signal is the bare null sentinel, not a string/kind object");
  assert(
    typeof result !== "string" && !(result && typeof result === "object" && "kind" in (result as object)),
    "parseCodexUsage's return value never carries a TIMEOUT/PARSE_ERROR-shaped label of its own",
  );

  // fetchCodexUsage (the quota-free API path) is the other half of this
  // file; its `null` on a failed/absent fetch is the input to a SEPARATE
  const missingAuth = fetchCodexUsage({
    credentialPublic: () => null,
    fetch: () => "",
  });
  assert(missingAuth === null, "fetchCodexUsage returns null (not a thrown timeout) when credentials are unavailable");
}

// The plugin never touches ~/.codex/auth.json: it names the credential and the
// header shape, and the host fills the bearer in. A confined bundle therefore
{
  let request: Record<string, unknown> | null = null;
  const raw = fetchCodexUsage({
    credentialPublic: (id: string) =>
      id === "codexAuth" ? JSON.stringify({ accountId: "account-1" }) : null,
    fetch: (opts: string) => {
      request = JSON.parse(opts);
      return JSON.stringify({
        status: 200,
        body: JSON.stringify({
          rate_limit: {
            primary_window: { used_percent: 17, limit_window_seconds: 18_000, reset_at: 1_780_000_000 },
            secondary_window: { used_percent: 92, limit_window_seconds: 604_800, reset_at: 1_781_000_000 },
          },
        }),
      });
    },
  });
  const headers = (request?.headers ?? {}) as Record<string, string>;
  const credential = request?.credential as { id?: string; headers?: Record<string, string> } | undefined;
  assert(raw !== null, "a resolvable credential produces a usage payload");
  assert(!("Authorization" in headers), "the plugin never composes an Authorization header itself");
  assert(credential?.id === "codexAuth", "the plugin names the declared credential instead of reading it");
  assert(
    (credential?.headers?.Authorization ?? "").indexOf("{accessToken}") !== -1,
    "the bearer stays an unfilled template on the plugin side",
  );
  assert(String(raw).indexOf("accessToken") === -1, "no credential material reaches the stored payload");
}

console.log(`\n${passed} passed, ${failed} failed\n`);
if (failed > 0) process.exit(1);
