import type { Notification } from "./types";
import type { AnswerUpdate } from "./notificationAnswer";
import { TransportError } from "./transportClient";
import { MAX_BELL_NOTIFICATIONS } from "./notificationAdmission";

export interface AnswerResult {
  answered?: string;
  delivered?: boolean;
  pending?: boolean;
  submit_unconfirmed?: boolean;
  deliveryError?: string;
  refusal?: { reason: string; detail?: string };
}

export function notificationAnswerResult(result: AnswerResult & { answered: string }): AnswerUpdate {
  if (result.pending || result.submit_unconfirmed) return { unconfirmed: true, answered: result.answered };
  return {
    answered: result.answered,
    delivered: result.delivered === true,
    error: result.deliveryError ?? result.refusal?.detail ?? result.refusal?.reason,
  };
}

export async function sendNotificationAnswer(
  id: string,
  label: string,
  pending: Set<string>,
  notification: Notification | undefined,
  send: (id: string, label: string) => Promise<AnswerResult>,
  update: (id: string, update: AnswerUpdate) => void,
): Promise<void> {
  if (pending.has(id) || notification?.answer_pending || notification?.answer_unconfirmed || notification?.answer?.delivered) return;
  if (pending.size >= MAX_BELL_NOTIFICATIONS) {
    update(id, { error: "Too many notification answers are in progress. Try again." });
    return;
  }
  pending.add(id);
  update(id, { pending: true });
  try {
    const result = await send(id, label);
    update(id, result.answered ? notificationAnswerResult({ ...result, answered: result.answered }) : { unconfirmed: true });
  } catch (error: unknown) {
    if (error instanceof TransportError && [400, 403, 404, 409].includes(error.status)) {
      update(id, { error: String(error) });
    } else {
      update(id, { unconfirmed: true });
    }
  } finally {
    pending.delete(id);
  }
}
