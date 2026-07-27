// Run: npx tsx src/components/Terminal/TabSessionChat.waiting-notification.test.ts

import { WaitingQuestionTracker } from "./waitingQuestionTracker";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function run(): void {
  const tracker = new WaitingQuestionTracker();
  const tabId = "tab-1";
  const sessionId = "session-owned-by-tab";
  let notifyCalls = 0;
  const observe = (
    state: "working" | "idle" | "question" | null,
    session: string | null = sessionId,
    hasCompleteMessage = true,
  ) =>
    tracker.observeAndNotify(tabId, session, state, hasCompleteMessage, () => {
      notifyCalls += 1;
    });

  const awaitingMessage = observe("question", sessionId, false);
  assert(awaitingMessage === 1, "authoritative question starts generation before its message arrives");
  assert(notifyCalls === 0, "incomplete message is never admitted to the inbox");

  const first = observe("question");
  assert(first === 1, "first authoritative question starts generation one");
  assert(notifyCalls === 1, "first generation emits one notification");

  const repeated = observe("question");
  assert(repeated === 1, "repaint keeps the same generation");
  assert(notifyCalls === 1, "repaint does not notify again");

  const remounted = observe("question");
  assert(remounted === 1, "remount keeps the same generation");
  assert(notifyCalls === 1, "remount does not notify again");

  const reconnect = observe(null);
  assert(reconnect === 1, "reconnect keeps the current generation");
  assert(notifyCalls === 1, "reconnect does not notify");
  const reconnectedQuestion = observe("question");
  assert(reconnectedQuestion === 1, "reconnected question stays in generation one");
  assert(notifyCalls === 1, "reconnected question does not notify again");

  observe("working");
  const second = observe("question");
  assert(second === 2, "resolved question followed by a new question advances generation");
  assert(notifyCalls === 2, "new generation emits a second notification");

  const otherSession = observe("question", "different-session");
  assert(otherSession === 1, "tab-owned sessions have independent generations");
  assert(notifyCalls === 3, "a different claimed session can notify once");

  assert(observe("question", null) === null, "unclaimed state never notifies");
  assert(notifyCalls === 3, "unclaimed state emits no notification");
  console.log("✓ TabSessionChat waiting notification generation test green");
}

run();
