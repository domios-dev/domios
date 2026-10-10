// Run: npx tsx src/components/Terminal/bootOverlay.test.tsx

import { tabBootOverlayView } from "./bootOverlay";
import type { Tab } from "../../lib/types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function tab(
  stage: Tab["boot_stage"],
  reason: string | null = null,
  agentType: string | null = null,
  protectedTab = false,
): Tab {
  return {
    id: "fixture-tab",
    pty_id: null,
    title: "fixture",
    protected: protectedTab,
    agent_type: agentType,
    boot_stage: stage,
    boot_failure_reason: reason,
  } as unknown as Tab;
}

assert(tabBootOverlayView(tab("pty_starting")).kind === "starting", "launch stage shows progress");
assert(tabBootOverlayView(tab("awaiting_owner_prompt")).kind === "starting", "owner prompt shows progress");
assert(tabBootOverlayView(tab("agent_ready")).kind === "none", "ready state has no overlay");

const unresolved = tabBootOverlayView(
  tab("spawn_failed", "opaque-reason-17"),
);
assert(unresolved.kind === "failed", "unresolved restore shows failure view");
assert(unresolved.reason === "opaque-reason-17", "failure reason passes through unchanged");
assert(unresolved.canStartFreshAgent, "providerless restore offers fresh provider selection");
assert(unresolved.canKeepShell, "failure view offers shell continuation");
assert(!unresolved.canRetry, "providerless restore does not guess a retry provider");

const retryable = tabBootOverlayView(
  tab("spawn_failed", "opaque-reason-18", "fixture-provider", true),
);
assert(retryable.kind === "failed" && retryable.canRetry, "identified protected agent may retry");
assert(!retryable.canStartFreshAgent, "identified provider is not replaced implicitly");

assert(
  tabBootOverlayView(tab("spawn_failed", "opaque-reason-17"), true).kind === "none",
  "keep-shell choice restores the terminal view",
);

console.log("✓ bootOverlay: boot state maps to view and available actions");
