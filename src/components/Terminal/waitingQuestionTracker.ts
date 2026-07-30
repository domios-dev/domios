import type { AgentStateKind } from "./AgentStateIndicator";

interface WaitingQuestionEntry {
  generation: number;
  promptIdentity: string | null;
  notified: boolean;
}

export interface WaitingQuestionObservation {
  generation: number;
  shouldNotify: boolean;
}

export interface InputRequestAdmission {
  promptIdentity: string | null;
  notificationReady: boolean;
}

// backendState.question is diagnostic only and deliberately not an input here:
// only the authoritative pending_prompt may mint notification identity.
export function deriveInputRequestAdmission(
  pendingPromptQuestion: string | null | undefined,
  state: AgentStateKind | null | undefined,
  subtype: string | null | undefined,
): InputRequestAdmission {
  const promptIdentity = pendingPromptQuestion?.trim() || null;
  const actionable = state === "question" && subtype !== "rate_limit" && subtype !== "error";
  return { promptIdentity, notificationReady: actionable && promptIdentity !== null };
}

export class WaitingQuestionTracker {
  private readonly entries = new Map<string, WaitingQuestionEntry>();

  resolve(tabId: string, sessionId: string | null, promptIdentity: string): void {
    if (!sessionId) return;
    const entry = this.entries.get(`${tabId}\0${sessionId}`);
    const identity = promptIdentity.trim();
    if (!entry || !identity || entry.promptIdentity !== identity) return;
    entry.promptIdentity = null;
    entry.notified = false;
  }

  observeAndNotify(
    tabId: string,
    sessionId: string | null,
    state: AgentStateKind | null,
    promptIdentity: string | null,
    notificationReady: boolean,
    notify: () => void,
  ): number | null {
    const observation = this.observe(tabId, sessionId, state, promptIdentity, notificationReady);
    if (observation?.shouldNotify) notify();
    return observation?.generation ?? null;
  }

  observe(
    tabId: string,
    sessionId: string | null,
    state: AgentStateKind | null,
    promptIdentity: string | null,
    notificationReady = true,
  ): WaitingQuestionObservation | null {
    if (!sessionId) return null;
    const key = `${tabId}\0${sessionId}`;
    const entry = this.entries.get(key);

    if (state == null) {
      return entry?.promptIdentity
        ? { generation: entry.generation, shouldNotify: false }
        : null;
    }
    const identity = promptIdentity?.trim() || null;
    if (state !== "question" || !identity) return null;

    if (entry?.promptIdentity === identity) {
      const shouldNotify = notificationReady && !entry.notified;
      if (shouldNotify) entry.notified = true;
      return { generation: entry.generation, shouldNotify };
    }

    const generation = (entry?.generation ?? 0) + 1;
    this.entries.set(key, {
      generation,
      promptIdentity: identity,
      notified: notificationReady,
    });
    return { generation, shouldNotify: notificationReady };
  }
}

export const waitingQuestionTracker = new WaitingQuestionTracker();
