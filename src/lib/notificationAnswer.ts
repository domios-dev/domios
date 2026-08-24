import type { NotificationAnswer } from "./types";

/**
 * Unanswered, with no tab left to carry an answer back. The card says so rather
 * than offering buttons that would quietly do nothing.
 */
export function isAnswerUndeliverable(answer: NotificationAnswer): boolean {
  return answer.answered === null && answer.asked_from_tab === null;
}

/**
 * An answer stands that never reached the tab that asked. The card says so
 * rather than showing it as answered-and-delivered.
 */
export function isAnswerUndelivered(answer: NotificationAnswer): boolean {
  return answer.answered !== null && !answer.delivered;
}
