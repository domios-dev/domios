// Run: npx tsx src/lib/restoreFailurePresentation.test.ts

import { restoreFailurePresentation } from "./restoreFailurePresentation";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const legacyRoleShell = restoreFailurePresentation(
  { bootStage: "spawn_failed", agentType: null, protectedTab: false },
  "opaque-reason-21",
  false,
);
assert(legacyRoleShell.view === "launch_failure", "unresolved restore opens failure view");
assert(legacyRoleShell.showFailureBadge, "unresolved restore has sidebar failure badge");
assert(!legacyRoleShell.showIdentityBadges, "failure does not claim a live role or model");
assert(legacyRoleShell.actions.startFreshAgent, "providerless restore offers provider selection");
assert(legacyRoleShell.actions.keepShell, "failure view offers shell continuation");
assert(legacyRoleShell.reason === "opaque-reason-21", "restore reason passes through unchanged");

const keptShell = restoreFailurePresentation(
  { bootStage: "spawn_failed", agentType: null, protectedTab: false },
  "opaque-reason-22",
  true,
);
assert(keptShell.view === "terminal", "keep-shell choice restores the terminal view");
assert(keptShell.showFailureBadge, "keeping shell does not hide the sidebar failure");
assert(!keptShell.actions.keepShell, "kept shell does not offer the same choice again");

const retryableAgent = restoreFailurePresentation(
  { bootStage: "spawn_failed", agentType: "fixture-provider", protectedTab: true },
  "reason",
  false,
);
assert(retryableAgent.actions.retry, "identified protected agent can retry");
assert(!retryableAgent.actions.startFreshAgent, "identified provider is not replaced by a guess");

const healthyRole = restoreFailurePresentation(
  { bootStage: "agent_ready", agentType: "fixture-provider", protectedTab: false },
  null,
  false,
);
assert(healthyRole.view === "terminal", "healthy tab keeps its view");
assert(!healthyRole.showFailureBadge, "healthy tab has no failure badge");
assert(healthyRole.showIdentityBadges, "healthy tab keeps its identity badges");

console.log("✓ restoreFailurePresentation: restore view, badge, identity, and action decisions");
