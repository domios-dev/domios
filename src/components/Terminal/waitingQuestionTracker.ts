import type { AgentStateKind } from "./AgentStateIndicator";
import type { PendingPrompt } from "../../lib/types";

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

const ACTIONABLE_SUBTYPES = new Set(["model_choice", "model_picker", "permission", "auth", "update"]);

// Effect dependency key over every admission input: changes exactly when a
// re-observation is required (e.g. actuatable options arrive under the same Q).
export function admissionDependencyKey(
  pendingPrompt: Pick<PendingPrompt, "question" | "options" | "submit_available"> | null | undefined,
  state: AgentStateKind | null | undefined,
  subtype: string | null | undefined,
): string {
  const admission = deriveInputRequestAdmission(pendingPrompt, state, subtype);
  return `${state ?? ""}\0${subtype ?? ""}\0${admission.promptIdentity ?? ""}\0${admission.notificationReady}`;
}

// Only a pending_prompt with an actuatable control (options or explicit submit)
// may mint notification identity; backendState.question is diagnostic only.
export function deriveInputRequestAdmission(
  pendingPrompt: Pick<PendingPrompt, "question" | "options" | "submit_available"> | null | undefined,
  state: AgentStateKind | null | undefined,
  subtype: string | null | undefined,
): InputRequestAdmission {
  const promptIdentity = pendingPrompt?.question.trim() || null;
  const actionable = state === "question" && ACTIONABLE_SUBTYPES.has(subtype ?? "");
  const actuatable = (pendingPrompt?.options.length ?? 0) > 0 || pendingPrompt?.submit_available === true;
  return { promptIdentity, notificationReady: actionable && actuatable && promptIdentity !== null };
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
    notify: () => Promise<void> | void,
  ): number | null {
    const observation = this.observe(tabId, sessionId, state, promptIdentity, notificationReady);
    if (observation?.shouldNotify) {
      const outcome = notify();
      // notified means confirmed delivery: a rejected attempt re-arms the
      // entry so the next observation retries (event-driven, no timers).
      if (outcome && typeof outcome.then === "function") {
        const key = `${tabId}\0${sessionId}`;
        const identity = promptIdentity?.trim() || null;
        outcome.catch(() => {
          const entry = this.entries.get(key);
          if (entry && entry.promptIdentity === identity) entry.notified = false;
        });
      }
    }
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
