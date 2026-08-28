import {
  initialSessionQuietState,
  reduceSessionQuiet,
  sessionQuietEventFromChat,
  type SessionQuietEvent,
} from "./sessionQuietDetection";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function reduce(state: ReturnType<typeof initialSessionQuietState>, event: SessionQuietEvent) {
  return reduceSessionQuiet(state, event);
}

function status(tabId: string, value: "Working" | "Waiting" | "Idle" | "Dead"): SessionQuietEvent {
  return { kind: "tab-status", tabId, status: value };
}

function growth(tabId: string, sessionId: string, eventTabId = tabId): SessionQuietEvent {
  return { kind: "transcript-growth", tabId: eventTabId, sourceSessionId: sessionId, hasMessages: true };
}

function defectTest(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduce(state, growth("tab-a", "session-a"));
  state = reduce(state, status("tab-a", "Working"));
  state = reduce(state, status("tab-a", "Working"));
  assert(state.quiet, "own session that stops growing while non-idle surfaces quiet");
}

function contaminationNegativeControl(): void {
  let first = initialSessionQuietState("tab-a", "session-a");
  let second = initialSessionQuietState("tab-b", "session-b");
  first = reduce(first, growth("tab-a", "session-a"));
  first = reduce(first, status("tab-a", "Working"));
  first = reduce(first, status("tab-a", "Working"));
  second = reduce(second, growth("tab-b", "session-b"));
  second = reduce(second, status("tab-b", "Working"));
  assert(first.quiet, "first tab keeps its own quiet verdict");
  assert(!second.quiet, "second tab's live growth stays non-quiet");
  assert(first.quiet && !second.quiet, "same-cwd sibling activity does not change either answer");
}

function idleTabStaysSilent(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduce(state, growth("tab-a", "session-a"));
  state = reduce(state, status("tab-a", "Working"));
  state = reduce(state, status("tab-a", "Idle"));
  assert(!state.quiet, "idle tab stays silent after a healthy session");
}

function unknownStaysSilent(): void {
  let state = initialSessionQuietState("tab-a", null);
  state = reduce(state, growth("tab-a", "session-from-somewhere-else"));
  state = reduce(state, status("tab-a", "Working"));
  state = reduce(state, status("tab-a", "Waiting"));
  assert(!state.quiet, "unbound tab never guesses a session identity");
}

function eventDrivesDetection(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduce(state, growth("tab-a", "session-a"));
  assert(!state.quiet, "growth alone does not claim the session is quiet");
  state = reduce(state, status("tab-a", "Working"));
  assert(!state.quiet, "the first explicit status event does not claim the session is quiet");
  state = reduce(state, status("tab-a", "Working"));
  assert(state.quiet, "the next explicit status event performs the quiet transition");

  const chatEvent = sessionQuietEventFromChat("tab-a", {
    tab_id: "tab-a",
    messages: [{ id: "m", type: "assistant", content: "reply" }],
    source_session_id: "session-a",
    session_binding_nonce: null,
  });
  assert(chatEvent.kind === "transcript-growth" && chatEvent.hasMessages, "transcript event is the growth input");
}

const tests = [
  ["session_quiet_detection surfaces own stopped session", defectTest],
  ["session_quiet_detection isolates same-cwd tabs", contaminationNegativeControl],
  ["session_quiet_detection keeps idle tabs silent", idleTabStaysSilent],
  ["session_quiet_detection stays silent when identity is unknown", unknownStaysSilent],
  ["session_quiet_detection is driven by explicit events", eventDrivesDetection],
] as const;

for (const [name, test] of tests) {
  try {
    test();
    console.log(`PASS ${name}`);
  } catch (error) {
    console.error(`FAIL ${name}`);
    throw error;
  }
}
