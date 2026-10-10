// Run: npx tsx src/components/Usage/UsageTable.test.ts

import { usageFailureCopy } from "./UsageTable";
import type { UsageSnapshot } from "../../types/usage";

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function row(over: Partial<UsageSnapshot>): UsageSnapshot {
  return {
    id: 1,
    provider: "claude",
    account_id: null,
    captured_at: 1_000,
    parsed_ok: false,
    billing_mode: "unknown",
    limits_reported: false,
    session_pct: null,
    session_resets_at: null,
    weekly_pct: null,
    weekly_resets_at: null,
    spend_cents: null,
    spend_limit_cents: null,
    spend_remaining_cents: null,
    spend_resets_at: null,
    failure_reason: null,
    probe_command: null,
    ...over,
  };
}

const codex = usageFailureCopy(row({
  provider: "codex",
  failure_reason: "output never reached parseable state",
  probe_command: "/status",
}));
assert(codex.providerLabel === "Codex", "Codex provider label");
assert(codex.command === "/status", "Codex uses /status");
assert(codex.reason.includes("parseable"), "Codex reason preserved");

const claude = usageFailureCopy(row({
  provider: "claude",
  failure_reason: "usage panel still loading after 22000ms",
  probe_command: "/usage",
}));
assert(claude.providerLabel === "Claude", "Claude provider label");
assert(claude.command === "/usage", "Claude uses /usage");
assert(claude.reason.includes("22000ms"), "Claude reason preserved");

const fallback = usageFailureCopy(row({ provider: "codex" }));
assert(fallback.command === "/status", "Codex fallback command is /status");

console.log("UsageTable: all passed");
