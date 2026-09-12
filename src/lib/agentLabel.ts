/**
 * The one agent-liveness observation and the one bracket-label rule, mirrored
 * from `codeterm_types::pty::agent_liveness` so the sidebar, the footer and the
 * CLI cannot disagree about the same tab. Every input is a fact the daemon
 * publishes; elapsed time is not one of them, and a tab that has produced no
 * fact yet is "unobserved" rather than guessed in either direction.
 */

export type AgentLiveness = "live" | "absent" | "unobserved";

export interface LivenessEvidence {
  /** The tab record claims an agent. A record is never evidence of a process. */
  declaresAgent: boolean;
  /** `null` when the tab names no PTY yet, so nothing has been observed. */
  ptyLive: boolean | null;
  /** Set for chatBackend tabs; the PTY fields are then ignored. */
  chatSessionOpen?: boolean | null;
  bootStage?: string | null;
  bootInFlight?: boolean;
}

/** Stages that are direct evidence the provider process is running. */
const PROVEN_UP = new Set(["provider_ready", "agent_ready"]);

/** Stages during which a launch is demonstrably still in flight. */
const LAUNCH_IN_FLIGHT = new Set([
  "agent_launching",
  "launch_written",
  "preparing_role_instructions",
  "delivering_instructions",
]);

export function classifyAgentLiveness(evidence: LivenessEvidence): AgentLiveness {
  if (!evidence.declaresAgent) return "absent";
  if (evidence.chatSessionOpen != null) {
    return evidence.chatSessionOpen ? "live" : "absent";
  }
  if (evidence.ptyLive == null) return "unobserved";
  if (!evidence.ptyLive) return "absent";

  const stage = evidence.bootStage ?? null;
  if (stage != null && PROVEN_UP.has(stage)) return "live";
  if (stage === "spawn_failed") return "absent";
  // A re-attached PTY proves the PTY, not the agent inside it, and an observer
  // giving up on a launch is not evidence the process it gave up on died.
  if (
    stage === "pty_reattached" ||
    stage === "launch_deadline_elapsed" ||
    stage === "launch_binding_unconfirmed" ||
    stage == null
  ) {
    return "unobserved";
  }
  if (evidence.bootInFlight || LAUNCH_IN_FLIGHT.has(stage)) return "unobserved";
  return "absent";
}

/**
 * THE bracket rule: the tab's agent IDENTITY as `provider:model`, never its
 * role. An unobserved component is omitted rather than guessed, and the
 * process verdict rides as a suffix so a label can never claim a running agent
 * that is not there: live -> `provider:model`, unobserved -> `provider:model?`,
 * absent -> `provider:model!`. No provider means no bracket.
 */
export function agentBracketLabel(
  provider: string | null | undefined,
  model: string | null | undefined,
  liveness: AgentLiveness,
): string | null {
  const providerText = provider?.trim();
  if (!providerText) return null;
  const modelText = model?.trim();
  const identity = modelText ? `${providerText}:${modelText}` : providerText;
  if (liveness === "live") return identity;
  return liveness === "absent" ? `${identity}!` : `${identity}?`;
}
