import {
  initialSessionQuietState,
  reduceSessionQuiet,
  sessionQuietEventFromChat,
  type SessionQuietEvent,
} from "./sessionQuietDetection";

// Matches output_feed::emit_tab_status_if_changed: adjacent status values cannot be equal.

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function reduce(state: ReturnType<typeof initialSessionQuietState>, event: SessionQuietEvent) {
  return reduceSessionQuiet(state, event);
}

type StatusValue = "Working" | "Waiting" | "Idle" | "Dead";

function status(tabId: string, value: StatusValue): SessionQuietEvent {
  return { kind: "tab-status", tabId, status: value };
}

function statusSequence(tabId: string, values: StatusValue[]): SessionQuietEvent[] {
  const events: SessionQuietEvent[] = [];
  let previous: StatusValue | undefined;
  for (const value of values) {
    if (value === previous) throw new Error(`invalid repeated status: ${value}`);
    events.push(status(tabId, value));
    previous = value;
  }
  return events;
}

function reduceStatuses(
  state: ReturnType<typeof initialSessionQuietState>,
  tabId: string,
  values: StatusValue[],
) {
  return statusSequence(tabId, values).reduce(reduce, state);
}

function growth(tabId: string, sessionId: string, eventTabId = tabId): SessionQuietEvent {
  return { kind: "transcript-growth", tabId: eventTabId, sourceSessionId: sessionId, hasMessages: true };
}

function defectTest(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduceStatuses(state, "tab-a", ["Working", "Waiting", "Working"]);
  assert(state.quiet, "bound session that completes Working without own growth surfaces quiet when work resumes");
}

function contaminationNegativeControl(): void {
  let first = initialSessionQuietState("tab-a", "session-a");
  let second = initialSessionQuietState("tab-b", "session-b");
  first = reduceStatuses(first, "tab-a", ["Working", "Waiting", "Working"]);
  second = reduceStatuses(second, "tab-b", ["Working"]);
  second = reduce(second, growth("tab-b", "session-b"));
  assert(first.quiet, "first tab keeps its own quiet verdict");
  assert(!second.quiet, "second tab's live growth stays non-quiet");
  assert(first.quiet && !second.quiet, "same-cwd sibling activity does not change either answer");
}

function idleTabStaysSilent(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduceStatuses(state, "tab-a", ["Working", "Idle"]);
  assert(!state.quiet, "idle tab stays silent after a healthy session");
}

function unknownStaysSilent(): void {
  let state = initialSessionQuietState("tab-a", null);
  state = reduce(state, growth("tab-a", "session-from-somewhere-else"));
  state = reduceStatuses(state, "tab-a", ["Working", "Waiting"]);
  assert(!state.quiet, "unbound tab never guesses a session identity");
}

function eventDrivesDetection(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduceStatuses(state, "tab-a", ["Working"]);
  assert(!state.quiet, "Working alone does not claim the session is quiet");
  state = reduce(state, status("tab-a", "Waiting"));
  assert(!state.quiet, "the Working to Waiting transition records evidence without claiming quiet");
  state = reduce(state, status("tab-a", "Working"));
  assert(state.quiet, "resumed Working consumes the completed silent-turn evidence");

  const chatEvent = sessionQuietEventFromChat("tab-a", {
    tab_id: "tab-a",
    messages: [{ id: "m", type: "assistant", content: "reply" }],
    source_session_id: "session-a",
    session_binding_nonce: null,
  });
  assert(chatEvent.kind === "transcript-growth" && chatEvent.hasMessages, "transcript event is the growth input");
}

function healthyTurnStaysSilentAtEveryStep(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduce(state, growth("tab-a", "session-a"));
  assert(!state.quiet, "healthy reply growth stays silent");
  state = reduce(state, status("tab-a", "Waiting"));
  assert(!state.quiet, "resting Waiting stays silent");
  state = reduce(state, status("tab-a", "Working"));
  assert(!state.quiet, "the first Working after rest stays silent");
  state = reduce(state, growth("tab-a", "session-a"));
  assert(!state.quiet, "the next healthy reply growth stays silent");
}

function workingWithoutOwnGrowthSurfacesQuiet(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduceStatuses(state, "tab-a", ["Working"]);
  assert(!state.quiet, "turn start is not evidence of a quiet session");
  state = reduce(state, status("tab-a", "Waiting"));
  assert(!state.quiet, "a completed Working turn records evidence without a false rest-state alert");
  state = reduce(state, status("tab-a", "Working"));
  assert(state.quiet, "a resumed Working turn without own growth is quiet");
}

function frostSequenceSurfacesQuiet(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduceStatuses(state, "tab-a", ["Working", "Waiting"]);
  assert(!state.quiet, "the first empty turn records evidence without alerting at rest");
  state = reduce(state, status("tab-a", "Working"));
  assert(state.quiet, "a second turn without growth surfaces quiet while Working");
  state = reduce(state, status("tab-a", "Waiting"));
  assert(state.quiet, "the quiet verdict remains visible after the second empty turn rests");
}

function frostSequenceWithOwnGrowthStaysSilent(): void {
  let state = initialSessionQuietState("tab-a", "session-a");
  state = reduce(state, status("tab-a", "Working"));
  assert(!state.quiet, "the first Working event stays silent");
  state = reduce(state, growth("tab-a", "session-a"));
  assert(!state.quiet, "own growth keeps the first turn silent");
  state = reduce(state, status("tab-a", "Waiting"));
  assert(!state.quiet, "a completed turn with own growth stays silent");
  state = reduce(state, status("tab-a", "Working"));
  assert(!state.quiet, "the next Working event stays silent");
  state = reduce(state, growth("tab-a", "session-a"));
  assert(!state.quiet, "the next own growth keeps the session silent");
  state = reduce(state, status("tab-a", "Waiting"));
  assert(!state.quiet, "the second completed turn with own growth stays silent");
}

function transitionConstraintRejectsRepeatedStatus(): void {
  let rejected = false;
  try {
    statusSequence("tab-a", ["Working", "Working"]);
  } catch {
    rejected = true;
  }
  assert(rejected, "tests reject status sequences the production emitter cannot emit");
  assert(statusSequence("tab-a", ["Working", "Waiting", "Working"]).length === 3, "valid transitions remain usable");
}

const tests = [
  ["session_quiet_detection surfaces a completed Working turn without own growth", defectTest],
  ["session_quiet_detection isolates same-cwd tabs", contaminationNegativeControl],
  ["session_quiet_detection keeps idle tabs silent", idleTabStaysSilent],
  ["session_quiet_detection stays silent when identity is unknown", unknownStaysSilent],
  ["session_quiet_detection is driven by explicit events", eventDrivesDetection],
  ["session_quiet_detection keeps a full healthy turn silent at every step", healthyTurnStaysSilentAtEveryStep],
  ["session_quiet_detection surfaces a Working turn without own growth", workingWithoutOwnGrowthSurfacesQuiet],
  ["session_quiet_detection reaches quiet on the Frost status sequence", frostSequenceSurfacesQuiet],
  ["session_quiet_detection keeps the Frost sequence silent with own growth", frostSequenceWithOwnGrowthStaysSilent],
  ["session_quiet_detection rejects impossible repeated status transitions", transitionConstraintRejectsRepeatedStatus],
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
