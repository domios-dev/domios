import type { AgentStateKind } from "./AgentStateIndicator";

interface WaitingQuestionEntry {
  generation: number;
  question: boolean;
  notified: boolean;
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
    notificationReady: boolean,
    notify: () => void,
  ): number | null {
    const observation = this.observe(paneId, sessionId, state, notificationReady);
    if (observation?.shouldNotify) notify();
    return observation?.generation ?? null;
  }

  observe(
    paneId: string,
    sessionId: string | null,
    state: AgentStateKind | null,
    notificationReady = true,
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
      if (entry) {
        entry.question = false;
        entry.notified = false;
      }
      return null;
    }
    if (entry?.question) {
      const shouldNotify = notificationReady && !entry.notified;
      if (shouldNotify) entry.notified = true;
      return { generation: entry.generation, shouldNotify };
    }

    const generation = (entry?.generation ?? 0) + 1;
    this.entries.set(key, { generation, question: true, notified: notificationReady });
    return { generation, shouldNotify: notificationReady };
  }
}

export const waitingQuestionTracker = new WaitingQuestionTracker();
