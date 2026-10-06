export interface RestoreFailureFacts {
  bootStage: string | null | undefined;
  agentType: string | null | undefined;
  protectedTab: boolean;
}

export interface RestoreFailurePresentation {
  view: "terminal" | "launch_failure";
  showFailureBadge: boolean;
  showIdentityBadges: boolean;
  reason: string | null;
  actions: {
    startFreshAgent: boolean;
    retry: boolean;
    keepShell: boolean;
  };
}

export function restoreFailurePresentation(
  facts: RestoreFailureFacts,
  failureReason: string | null | undefined,
  keepShell: boolean,
): RestoreFailurePresentation {
  const failed = facts.bootStage === "spawn_failed";
  return {
    view: failed && !keepShell ? "launch_failure" : "terminal",
    showFailureBadge: failed,
    showIdentityBadges: !failed,
    reason: failed ? failureReason ?? null : null,
    actions: {
      startFreshAgent: failed && facts.agentType == null,
      retry: failed && facts.protectedTab && facts.agentType != null,
      keepShell: failed && !keepShell,
    },
  };
}
