import type { Notification } from "../../lib/types";
import { isAnswerUndeliverable } from "../../lib/notificationAnswer";

export type ChatAskPhase = "open" | "sending" | "answered" | "failed" | "resolved" | "undeliverable";

export interface ChatAskChoiceState {
  phase: ChatAskPhase;
  chosen: string | null;
  error: string | null;
  showChoices: boolean;
}

/**
 * One ask card's view of its record plus the label this surface just tapped.
 * The tap is optimistic: it shows as sending before the store has heard of it,
 * and the record wins as soon as it carries an answer, a pending flag or an error.
 */
export function chatAskChoiceState(notification: Notification, tapped: string | null): ChatAskChoiceState {
  const slot = notification.answer;
  const recorded = slot?.answered ?? null;
  const chosen = recorded ?? tapped;
  if (notification.resolved) return { phase: "resolved", chosen: recorded, error: null, showChoices: false };
  if (notification.answer_pending) return { phase: "sending", chosen, error: null, showChoices: true };
  if (notification.answer_error) {
    return { phase: "failed", chosen, error: notification.answer_error, showChoices: true };
  }
  if (recorded !== null) return { phase: "answered", chosen: recorded, error: null, showChoices: false };
  if (slot && isAnswerUndeliverable(slot)) {
    return { phase: "undeliverable", chosen: null, error: null, showChoices: false };
  }
  if (tapped !== null) return { phase: "sending", chosen: tapped, error: null, showChoices: true };
  return { phase: "open", chosen: null, error: null, showChoices: true };
}

/** A failed answer that the store already holds can only be retried with that same label. */
export function isChoiceEnabled(state: ChatAskChoiceState, label: string, recorded: string | null): boolean {
  if (state.phase === "open") return true;
  if (state.phase !== "failed") return false;
  return recorded === null || recorded === label;
}

/** Whether a tap starts a new attempt; a tap while one is in flight or after it settled is a no-op. */
export function acceptsTap(state: ChatAskChoiceState, label: string, recorded: string | null): boolean {
  return state.showChoices && isChoiceEnabled(state, label, recorded);
}
