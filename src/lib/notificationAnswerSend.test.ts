import assert from "node:assert/strict";
import { sendNotificationAnswer, type AnswerResult } from "./notificationAnswerSend";
import { updateNotificationAnswer } from "./notificationAnswer";
import type { Notification } from "./types";

async function run() {
  let notification: Notification = {
    id: "question", tab_id: "asker", workspace_id: null, title: "Question",
    body: null, severity: "Info", timestamp: 0, read: false,
    answer: { asked_from_tab: "asker", answered: null, delivered: false },
  };
  const pending = new Set<string>();
  const update = (_id: string, result: Parameters<typeof updateNotificationAnswer>[1]) => {
    notification = updateNotificationAnswer(notification, result);
  };
  let finish!: (result: AnswerResult) => void;
  const response = new Promise<AnswerResult>((resolve) => { finish = resolve; });
  let calls = 0;
  const send = async () => { calls++; return response; };
  const first = sendNotificationAnswer("question", "A", pending, notification, send, update);
  assert.equal(notification.answer_pending, true);
  assert.equal(notification.answer?.answered, null);
  await Promise.all([
    sendNotificationAnswer("question", "A", pending, notification, send, update),
    sendNotificationAnswer("question", "B", pending, notification, send, update),
  ]);
  assert.equal(calls, 1);
  finish({ answered: "A", delivered: true });
  await first;
  assert.equal(notification.answer_pending, false);
  assert.equal(notification.answer?.delivered, true);
  await sendNotificationAnswer("question", "B", pending, notification, send, update);
  assert.equal(calls, 1);
  notification = updateNotificationAnswer(notification, { answered: "B", delivered: false });
  assert.equal(notification.answer?.answered, "A");
  assert.equal(notification.answer?.delivered, true);

  notification = { ...notification, answer: { asked_from_tab: "asker", answered: null, delivered: false } };
  await sendNotificationAnswer("question", "A", pending, notification, async () => { throw new Error("Tab unavailable"); }, update);
  assert.equal(notification.answer_pending, false);
  assert.match(notification.answer_error ?? "", /Tab unavailable/);
  assert.equal(pending.size, 0);
  await sendNotificationAnswer("question", "A", pending, notification, async () => ({ answered: "A", delivered: false, deliveryError: "No agent" }), update);
  assert.equal(notification.answer_pending, false);
  assert.equal(notification.answer_error, "No agent");
  await sendNotificationAnswer("question", "A", pending, notification, async () => ({ answered: "A", delivered: true }), update);
  assert.equal(notification.answer_error, undefined);
  assert.equal(notification.answer?.delivered, true);
  console.log("notification answer model and sender: 15 assertions passed");

}

void run();
