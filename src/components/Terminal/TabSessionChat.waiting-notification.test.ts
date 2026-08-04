// Run: npx tsx src/components/Terminal/TabSessionChat.waiting-notification.test.ts

import { WaitingQuestionTracker, admissionDependencyKey, deriveInputRequestAdmission } from "./waitingQuestionTracker";

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
  type TestPrompt = {
    question: string;
    options: Array<{ label: string; value: string }>;
    submit_available?: boolean | null;
  };
  const interactive = (question: string): TestPrompt => ({
    question,
    options: [{ label: "Yes", value: "1" }, { label: "No", value: "2" }],
  });
  const textOnly = (question: string): TestPrompt => ({ question, options: [] });
  const observe = (
    state: "working" | "idle" | "question" | null,
    subtype: string | null,
    pendingPrompt: TestPrompt | null,
  ) => {
    const admission = deriveInputRequestAdmission(pendingPrompt, state, subtype);
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

  const classifierOnly = observe("question", "model_choice", null);
  assert(classifierOnly === null, "backend question without pending_prompt yields no waiting generation");
  assert(Number(notifyCalls) === 0, "backend question without pending_prompt never notifies");

  // Prod P0 oracle 4ae14e7f (2026-07-30T17:39:40.457Z): a structurally-minted
  // prompt whose question text classifies "unknown" must never ring a bell.
  observe(
    "question",
    "unknown",
    interactive("⚠ D:\\Developer\\codeterm\\.agents\\skills\\gap-analysis\\SKILL.md: missing field description"),
  );
  assert(Number(notifyCalls) === 0, "unknown subtype never notifies");

  // Coordination envelopes keyword-match an actionable subtype ("model=…") but
  // carry no actuatable control — the control gate must reject them.
  observe(
    "question",
    "model_picker",
    textOnly("-=-codeterm:tab label=Reviewer tab=166df18d model=gpt-5.6-sol-=-Coordination note only."),
  );
  assert(Number(notifyCalls) === 0, "control-less envelope text never notifies");

  observe("question", "unknown", textOnly("gpt-5.6-sol high · D:\\Developer\\codeterm"));
  assert(Number(notifyCalls) === 0, "terminal chrome never notifies");

  // Live prod rows 340b2c5d / 2737d833 / d67eb8c1: tool chrome, CLI help, and
  // prose that keyword-matches an actionable subtype — all control-less.
  observe("question", "unknown", textOnly("Searched for 1 pattern (ctrl+o to expand)"));
  observe("question", "unknown", textOnly("h, --help Print help"));
  observe("question", "auth", textOnly("RE-ARMS it. provider-stop then resumes the readiness loop."));
  assert(Number(notifyCalls) === 0, "tool chrome, help text, and prose never notify");

  observe("question", null, interactive("An unclassified question prompt"));
  assert(Number(notifyCalls) === 0, "unclassified subtype fails closed");
  observe("working", null, null);

  const genuine = observe("question", "model_choice", interactive("Which deployment region?  "));
  assert(genuine === 8, "authoritative actionable prompt starts a fresh generation");
  assert(Number(notifyCalls) === 1, "authoritative actionable prompt notifies exactly once");

  const redraw = observe("question", "model_choice", interactive("Which deployment region?"));
  assert(redraw === 8, "repeated frames keep the generation");
  assert(Number(notifyCalls) === 1, "repeated frames do not duplicate the notification");

  const cleared = observe("working", null, null);
  assert(cleared === null, "cleared prompt ends the waiting generation");
  tracker.resolve(tabId, sessionId, "Which deployment region?");
  assert(Number(notifyCalls) === 1, "clear/resolve emits nothing");

  const rateLimit = observe("question", "rate_limit", interactive("Rate limited, retry?"));
  assert(rateLimit === 9, "rate_limit tracks state without a bell");
  const errored = observe("question", "error", interactive("Rate limited, retry?"));
  assert(errored === 9, "error tracks state without a bell");
  assert(Number(notifyCalls) === 1, "rate_limit and error states never notify");

  const freeText = observe("question", "model_choice", {
    question: "Deploy to which cluster?",
    options: [],
    submit_available: true,
  });
  assert(freeText === 10, "free-text prompt with explicit submit starts a generation");
  assert(Number(notifyCalls) === 2, "explicit submit control is actuatable and notifies once");

  console.log("✓ TabSessionChat input-request admission test green");
}

// Live false-negative on dev b3396a7c (tab f6d23bb4): genuine picker, both
// admission gates true, zero rows — one-shot invocation loss, orderings below.
async function runInvocationOrderings(): Promise<void> {
  const tabId = "tab-ord";
  const sessionId = "session-owned-by-tab";

  {
    // state-before-prompt: question verdict arrives before the prompt exists.
    const tracker = new WaitingQuestionTracker();
    let notifies = 0;
    const frames: Array<[string | null, ReturnType<typeof deriveInputRequestAdmission>]> = [
      ["question", deriveInputRequestAdmission(null, "question", "model_picker")],
      ["question", deriveInputRequestAdmission(
        { question: "Select Model and Effort", options: sevenOptions(), submit_available: true },
        "question",
        "model_picker",
      )],
    ];
    for (const [state, admission] of frames) {
      tracker.observeAndNotify(tabId, sessionId, state as never, admission.promptIdentity, admission.notificationReady, () => {
        notifies += 1;
      });
    }
    assert(notifies === 1, "state-before-prompt delivers exactly once");
  }

  {
    // prompt-before-state: actuatable prompt published while verdict still working.
    const tracker = new WaitingQuestionTracker();
    let notifies = 0;
    const prompt = { question: "Select Model and Effort", options: sevenOptions(), submit_available: true };
    for (const state of ["working", "question"] as const) {
      const admission = deriveInputRequestAdmission(prompt, state, "model_picker");
      tracker.observeAndNotify(tabId, sessionId, state, admission.promptIdentity, admission.notificationReady, () => {
        notifies += 1;
      });
    }
    assert(notifies === 1, "prompt-before-state delivers exactly once");
  }

  {
    // two-stage publication: same question, actuatable control arrives later.
    const tracker = new WaitingQuestionTracker();
    let notifies = 0;
    const stages = [
      { question: "Select Model and Effort", options: [] },
      { question: "Select Model and Effort", options: sevenOptions(), submit_available: true },
    ];
    for (const prompt of stages) {
      const admission = deriveInputRequestAdmission(prompt, "question", "model_picker");
      tracker.observeAndNotify(tabId, sessionId, "question", admission.promptIdentity, admission.notificationReady, () => {
        notifies += 1;
      });
    }
    assert(notifies === 1, "late-arriving actuatable control still delivers exactly once");
  }

  {
    // Rejected delivery must not burn the once-per-generation budget: the next
    // observation retries; a confirmed delivery stops further attempts.
    const tracker = new WaitingQuestionTracker();
    let attempts = 0;
    const prompt = { question: "Select Model and Effort", options: sevenOptions(), submit_available: true };
    const observeWith = (outcome: "reject" | "resolve") => {
      const admission = deriveInputRequestAdmission(prompt, "question", "model_picker");
      return tracker.observeAndNotify(tabId, sessionId, "question", admission.promptIdentity, admission.notificationReady, () => {
        attempts += 1;
        return outcome === "reject"
          ? Promise.reject(new Error("Tab is not waiting for actionable user input"))
          : Promise.resolve();
      });
    };
    observeWith("reject");
    await Promise.resolve();
    assert(Number(attempts) === 1, "first attempt fired");
    observeWith("resolve");
    await Promise.resolve();
    assert(Number(attempts) === 2, "a rejected delivery retries on the next observation");
    observeWith("resolve");
    assert(Number(attempts) === 2, "a confirmed delivery never repeats");
  }

  {
    // The effect dependency key must change whenever a re-observation is due:
    // this is the structural guard for the live f6d23bb4 loss (same-Q stages).
    const textOnly = { question: "Select Model and Effort", options: [] };
    const actuatable = { question: "Select Model and Effort", options: sevenOptions(), submit_available: true };
    assert(
      admissionDependencyKey(textOnly, "question", "model_picker")
        !== admissionDependencyKey(actuatable, "question", "model_picker"),
      "late-arriving options under the same question change the dependency key",
    );
    assert(
      admissionDependencyKey(null, "question", "model_picker")
        !== admissionDependencyKey(actuatable, "question", "model_picker"),
      "state-before-prompt publication changes the dependency key",
    );
    assert(
      admissionDependencyKey(actuatable, "working", "model_picker")
        !== admissionDependencyKey(actuatable, "question", "model_picker"),
      "prompt-before-state verdict flip changes the dependency key",
    );
    assert(
      admissionDependencyKey(actuatable, "question", "model_picker")
        === admissionDependencyKey({ ...actuatable, options: sevenOptions() }, "question", "model_picker"),
      "equivalent frames keep a stable key (no spurious re-observation)",
    );
  }

  console.log("✓ TabSessionChat invocation ordering test green");
}

function sevenOptions(): Array<{ label: string; value: string }> {
  return Array.from({ length: 7 }, (_, i) => ({ label: `Option ${i + 1}`, value: String(i + 1) }));
}

run();
runAdmission();
runInvocationOrderings().catch((error) => {
  console.error(error);
  process.exit(1);
});
