// Run: npx tsx src/components/Terminal/TabSessionChat.waiting-notification.test.ts

import { WaitingQuestionTracker, deriveInputRequestAdmission } from "./waitingQuestionTracker";

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
    promptIdentity: string | null = "Which deployment region?",
    hasCompleteMessage = true,
  ) =>
    tracker.observeAndNotify(tabId, session, state, promptIdentity, hasCompleteMessage, () => {
      notifyCalls += 1;
    });

  const awaitingMessage = observe("question", sessionId, "Which deployment region?", false);
  assert(awaitingMessage === 1, "authoritative question starts generation before its message arrives");
  assert(Number(notifyCalls) === 0, "incomplete message is never admitted to the inbox");

  const first = observe("question");
  assert(first === 1, "first authoritative question starts generation one");
  assert(Number(notifyCalls) === 1, "first generation emits one notification");

  const repeated = observe("question");
  assert(repeated === 1, "repaint keeps the same generation");
  assert(Number(notifyCalls) === 1, "repaint does not notify again");

  const remounted = observe("question");
  assert(remounted === 1, "remount keeps the same generation");
  assert(Number(notifyCalls) === 1, "remount does not notify again");

  const reconnect = observe(null);
  assert(reconnect === 1, "reconnect keeps the current generation");
  assert(Number(notifyCalls) === 1, "reconnect does not notify");
  const reconnectedQuestion = observe("question");
  assert(reconnectedQuestion === 1, "reconnected question stays in generation one");
  assert(Number(notifyCalls) === 1, "reconnected question does not notify again");

  observe("working", sessionId, null);
  const flapped = observe("question");
  assert(flapped === 1, "an identical prompt survives a transient question/working flap");
  assert(Number(notifyCalls) === 1, "an identical flapped prompt remains at most once");

  const second = observe("question", sessionId, "Choose a failover region");
  assert(second === 2, "a genuinely new prompt identity advances generation");
  assert(Number(notifyCalls) === 2, "a genuinely new prompt emits a second notification");

  tracker.resolve(tabId, sessionId, "Choose a failover region");
  const repeatedAfterResolution = observe("question", sessionId, "Choose a failover region");
  assert(repeatedAfterResolution === 3, "authoritative resolution ends the prior prompt event");
  assert(Number(notifyCalls) === 3, "the same prompt text can notify for a later resolved event");

  const otherSession = observe("question", "different-session");
  assert(otherSession === 1, "tab-owned sessions have independent generations");
  assert(Number(notifyCalls) === 4, "a different claimed session can notify once");

  assert(observe("question", null) === null, "unclaimed state never notifies");
  assert(Number(notifyCalls) === 4, "unclaimed state emits no notification");
  console.log("✓ TabSessionChat waiting notification generation test green");
}

function runAdmission(): void {
  const tracker = new WaitingQuestionTracker();
  const tabId = "tab-adm";
  const sessionId = "session-owned-by-tab";
  let notifyCalls = 0;
  const observe = (
    state: "working" | "idle" | "question" | null,
    subtype: string | null,
    pendingPromptQuestion: string | null,
  ) => {
    const admission = deriveInputRequestAdmission(pendingPromptQuestion, state, subtype);
    return tracker.observeAndNotify(
      tabId,
      sessionId,
      state,
      admission.promptIdentity,
      admission.notificationReady,
      () => {
        notifyCalls += 1;
      },
    );
  };

  const classifierOnly = observe("question", null, null);
  assert(classifierOnly === null, "backend question without pending_prompt yields no waiting generation");
  assert(Number(notifyCalls) === 0, "backend question without pending_prompt never notifies");

  const genuine = observe("question", null, "Which deployment region?  ");
  assert(genuine === 1, "authoritative pending prompt starts generation one");
  assert(Number(notifyCalls) === 1, "authoritative pending prompt notifies exactly once");

  const redraw = observe("question", null, "Which deployment region?");
  assert(redraw === 1, "repeated frames keep the generation");
  assert(Number(notifyCalls) === 1, "repeated frames do not duplicate the notification");

  const cleared = observe("working", null, null);
  assert(cleared === null, "cleared prompt ends the waiting generation");
  tracker.resolve(tabId, sessionId, "Which deployment region?");
  assert(Number(notifyCalls) === 1, "clear/resolve emits nothing");

  const rateLimit = observe("question", "rate_limit", "Rate limited, retry?");
  assert(rateLimit === 2, "rate_limit tracks state without a bell");
  const errored = observe("question", "error", "Rate limited, retry?");
  assert(errored === 2, "error tracks state without a bell");
  assert(Number(notifyCalls) === 1, "rate_limit and error states never notify");

  console.log("✓ TabSessionChat input-request admission test green");
}

run();
runAdmission();
