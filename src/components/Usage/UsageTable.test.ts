// Run: npx tsx src/components/Usage/UsageTable.test.ts

import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { UsageTable } from "./UsageTable";
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

async function accessibleUsageFailure() {
  const dom = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLInputElement", "HTMLButtonElement", "SVGElement", "Event", "MouseEvent", "Element", "Node"] as const) {
    Object.defineProperty(globalThis, key, { value: dom[key], configurable: true });
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(UsageTable, { rows: [row({ failure_reason: "TIMEOUT: opaque probe result" })] })));
    assert(host.querySelector("details") === null, "background usage details are not shown as a notice");
    assert(host.querySelector("table tbody tr") !== null, "the last usage row stays available");
    assert(host.querySelector('[role="alert"]') === null, "background usage failure does not create an alert");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    await dom.happyDOM.abort();
  }
}

void accessibleUsageFailure().catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
