import type { AgentStateKind } from "./AgentStateIndicator";

interface WaitingQuestionEntry {
  generation: number;
  question: boolean;
}

export interface WaitingQuestionObservation {
  generation: number;
  shouldNotify: boolean;
}

export class WaitingQuestionTracker {
  private readonly entries = new Map<string, WaitingQuestionEntry>();

  observeAndNotify(
    paneId: string,
    sessionId: string | null,
    state: AgentStateKind | null,
    notify: () => void,
  ): number | null {
    const observation = this.observe(paneId, sessionId, state);
    if (observation?.shouldNotify) notify();
    return observation?.generation ?? null;
  }

  observe(
    paneId: string,
    sessionId: string | null,
    state: AgentStateKind | null,
  ): WaitingQuestionObservation | null {
    if (!sessionId) return null;
    const key = `${paneId}\0${sessionId}`;
    const entry = this.entries.get(key);

    if (state == null) {
      return entry?.question
        ? { generation: entry.generation, shouldNotify: false }
        : null;
    }
    if (state !== "question") {
      if (entry) entry.question = false;
      return null;
    }
    if (entry?.question) {
      return { generation: entry.generation, shouldNotify: false };
    }

    const generation = (entry?.generation ?? 0) + 1;
    this.entries.set(key, { generation, question: true });
    return { generation, shouldNotify: true };
  }
}

export const waitingQuestionTracker = new WaitingQuestionTracker();
