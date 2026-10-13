import type { Notification } from "./types";
import type { AnswerUpdate } from "./notificationAnswer";

export interface AnswerResult {
  answered?: string;
  delivered?: boolean;
  pending?: boolean;
  deliveryError?: string;
  refusal?: { reason: string; detail?: string };
}

export async function sendNotificationAnswer(
  id: string,
  label: string,
  pending: Set<string>,
  notification: Notification | undefined,
  send: (id: string, label: string) => Promise<AnswerResult>,
  update: (id: string, update: AnswerUpdate) => void,
): Promise<void> {
  if (pending.has(id) || notification?.answer_pending || notification?.answer?.delivered) return;
  pending.add(id);
  update(id, { pending: true });
  try {
    const result = await send(id, label);
    if (result.pending) return;
    if (!result.answered) throw new Error("The answer was not recorded. Try again.");
    update(id, {
      answered: result.answered,
      delivered: result.delivered === true,
      error: result.deliveryError ?? result.refusal?.detail ?? result.refusal?.reason,
    });
  } catch (error: unknown) {
    update(id, { error: String(error) });
  } finally {
    pending.delete(id);
  }
}
