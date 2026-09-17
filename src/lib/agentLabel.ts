/**
 * The one agent-liveness observation and the one bracket-label rule, mirrored
 * from `codeterm_types::pty::agent_liveness` so the sidebar, the footer and the
 * CLI cannot disagree about the same tab. Every input is a fact the daemon
 * publishes; elapsed time is not one of them, and a tab that has produced no
 * fact yet is "unobserved" rather than guessed in either direction.
 */

import { shortModelName } from "./agentModelName";

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

export interface AgentModelEvidence {
  /** The model detected from this tab's currently claimed provider session. */
  sessionModel?: string | null;
  /** The model resolved for this tab before its runtime session reports one. */
  providerResolvedModel?: string | null;
  /** The explicit model request for this tab, when resolution has not completed. */
  requestedModel?: string | null;
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

function nonEmptyModel(model: string | null | undefined): string | null {
  const trimmed = model?.trim() ?? "";
  return trimmed ? trimmed : null;
}

/** Selects only model evidence owned by the tab; runtime evidence wins launch provenance. */
export function agentModelForBadge(evidence: AgentModelEvidence): string | null {
  return (
    nonEmptyModel(evidence.sessionModel) ??
    nonEmptyModel(evidence.providerResolvedModel) ??
    nonEmptyModel(evidence.requestedModel)
  );
}

/**
 * THE bracket rule: the tab's agent IDENTITY, never its role. The model
 * REPLACES the provider, it never joins it, and exactly three cases exist:
 * no usable provider -> no bracket; provider with a model no catalogue claims
 * -> the provider name; a recognised model -> the short model name, alone.
 *
 * Falling back to the provider is a display decision about an unclaimed id; it
 * says nothing about the stored model, which this rule never writes.
 *
 * The process verdict rides as a suffix so a label can never claim a running
 * agent that is not there: live -> `sol`, unobserved -> `sol?`, absent -> `sol!`.
 */
export function agentBracketLabel(
  provider: string | null | undefined,
  model: string | null | undefined,
  liveness: AgentLiveness,
): string | null {
  const providerText = provider?.trim();
  if (!providerText) return null;
  const modelText = model?.trim();
  const identity = (modelText ? shortModelName(modelText) : null) ?? providerText;
  if (liveness === "live") return identity;
  return liveness === "absent" ? `${identity}!` : `${identity}?`;
}

/** The sidebar's single label path, including model evidence and liveness. */
export function agentBadgeLabel(
  provider: string | null | undefined,
  evidence: AgentModelEvidence,
  liveness: AgentLiveness,
): string | null {
  return agentBracketLabel(provider, agentModelForBadge(evidence), liveness);
}
