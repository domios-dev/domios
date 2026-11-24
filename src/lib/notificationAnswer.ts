import type { NotificationAnswer } from "./types";

/**
 * Unanswered, with no tab left to carry an answer back. The card says so rather
 * than offering buttons that would quietly do nothing.
 */
export function isAnswerUndeliverable(answer: NotificationAnswer): boolean {
  return answer.answered === null && answer.asked_from_tab === null && !answer.daemon_request;
}

/**
 * An answer stands that never reached the tab that asked. The card says so
 * rather than showing it as answered-and-delivered.
 */
export function isAnswerUndelivered(answer: NotificationAnswer): boolean {
  return answer.answered !== null && !answer.delivered;
}

export type AnswerUpdate =
  | { pending: true }
  | { unconfirmed: true; answered?: string }
  | { error: string }
  | { answered: string; delivered: boolean; error?: string };

export function updateNotificationAnswer(notification: import("./types").Notification, update: AnswerUpdate) {
  if (!notification.answer || notification.answer.delivered) return notification;
  if ("pending" in update) return { ...notification, answer_pending: true, answer_unconfirmed: false, answer_error: undefined };
  if ("unconfirmed" in update) {
    if (notification.answer_error) return notification;
    return {
      ...notification,
      read: notification.read || Boolean(notification.answer.answered ?? update.answered),
      answer_pending: false,
      answer_unconfirmed: true,
      answer: { ...notification.answer, answered: notification.answer.answered ?? update.answered ?? null },
    };
  }
  if (!("answered" in update)) return { ...notification, answer_pending: false, answer_unconfirmed: false, answer_error: update.error };
  return {
    ...notification,
    read: true,
    answer_pending: false,
    answer_unconfirmed: false,
    answer_error: update.delivered ? undefined : update.error ?? "The answer could not be delivered. Try again.",
    answer: {
      ...notification.answer,
      answered: notification.answer.answered ?? update.answered,
      delivered: update.delivered,
    },
  };
}
