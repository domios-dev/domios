import { describe, expect, it } from "vitest";
import type { HelperFacts } from "./helperBlocks";
import {
  EMPTY_TRACKER,
  MOMENT_THRESHOLDS,
  observeSignal,
  pluginForServiceRequest,
  repeatedSequence,
  type HelperSignal,
  type HelperTabKind,
  type MomentTracker,
} from "./momentRegistry";

const FACTS: HelperFacts = {
  telegramConnected: false,
  plugins: [
    { id: "jira-client", name: "Jira", installed: false },
    { id: "teams-client", name: "Teams Client", installed: true },
  ],
  blueprintsInUse: false,
};
const KINDS: Record<string, HelperTabKind> = { agent: "agent", shell: "manual-shell", utility: "other" };
const kind = (tabId: string) => KINDS[tabId] ?? "other";
const AWAY = MOMENT_THRESHOLDS.awayMs;
const LONG = MOMENT_THRESHOLDS.longRunMs;

function run(signals: HelperSignal[], facts = FACTS) {
  let tracker: MomentTracker = EMPTY_TRACKER;
  const hits = [];
  for (const signal of signals) {
    const result = observeSignal(tracker, signal, facts, kind);
    tracker = result.tracker;
    if (result.hit) hits.push(result.hit);
  }
  return hits;
}

describe("waiting_on_user_while_away", () => {
  it("fires on return when an agent waited through the whole away threshold", () => {
    const hits = run([
      { kind: "window-focus", focused: false, at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Waiting", at: 1_000 },
      { kind: "window-focus", focused: true, at: 1_000 + AWAY },
    ]);
    expect(hits.map((hit) => [hit.momentId, hit.block])).toEqual([["waiting_on_user_while_away", { kind: "guide", sectionId: "phone" }]]);
  });

  it("stays quiet for a short absence, a focused wait, or a non-agent tab", () => {
    expect(run([
      { kind: "window-focus", focused: false, at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Waiting", at: 0 },
      { kind: "window-focus", focused: true, at: AWAY - 1 },
    ])).toEqual([]);
    expect(run([
      { kind: "tab-status", tabId: "agent", status: "Waiting", at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Working", at: AWAY * 2 },
    ])).toEqual([]);
    expect(run([
      { kind: "window-focus", focused: false, at: 0 },
      { kind: "tab-status", tabId: "utility", status: "Waiting", at: 0 },
      { kind: "window-focus", focused: true, at: AWAY * 2 },
    ])).toEqual([]);
  });

  it("fires when the wait ends while the user is still away", () => {
    const hits = run([
      { kind: "tab-status", tabId: "agent", status: "Waiting", at: 0 },
      { kind: "window-focus", focused: false, at: 500 },
      { kind: "tab-status", tabId: "agent", status: "Working", at: 500 + AWAY },
    ]);
    expect(hits.map((hit) => hit.momentId)).toEqual(["waiting_on_user_while_away"]);
  });
});

describe("long_task_finished_unattended", () => {
  it("fires when a long run finishes while the window is unfocused", () => {
    const hits = run([
      { kind: "tab-status", tabId: "agent", status: "Working", at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Working", at: 100 },
      { kind: "window-focus", focused: false, at: 200 },
      { kind: "tab-status", tabId: "agent", status: "Idle", at: LONG },
    ]);
    expect(hits.map((hit) => [hit.momentId, hit.tabId])).toEqual([["long_task_finished_unattended", "agent"]]);
  });

  it("ignores short runs and runs that finish in front of the user", () => {
    expect(run([
      { kind: "window-focus", focused: false, at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Working", at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Idle", at: LONG - 1 },
    ])).toEqual([]);
    expect(run([
      { kind: "tab-status", tabId: "agent", status: "Working", at: 0 },
      { kind: "tab-status", tabId: "agent", status: "Idle", at: LONG * 2 },
    ])).toEqual([]);
  });
});

describe("repeated_manual_action", () => {
  const command = (commandLine: string, tabId = "shell", at = 0): HelperSignal => ({ kind: "shell-command", tabId, commandLine, at });

  it("fires on the third repetition of a command sequence in a manual shell", () => {
    const sequence = ["npm run build", "./deploy.sh  staging"];
    const signals = [...sequence, "git status", ...sequence, ...sequence].map((line) => command(line));
    const hits = run(signals);
    expect(hits.map((hit) => [hit.momentId, hit.block, hit.tabId])).toEqual([["repeated_manual_action", { kind: "guide", sectionId: "blueprints" }, "shell"]]);
  });

  it("ignores trivial commands, agent-driven shells and fewer than three repeats", () => {
    expect(repeatedSequence(["ls", "ls", "ls", "cd src"])).toBe(false);
    expect(repeatedSequence(["make test", "make test", "make test", "make test", "make test", "make test"])).toBe(false);
    expect(repeatedSequence(["cd a", "ls", "cd a", "ls", "cd a", "ls"])).toBe(false);
    expect(run(["make test", "make test", "make test"].map((line) => command(line, "utility")))).toEqual([]);
  });
});

describe("service_without_plugin", () => {
  it("offers the not-installed plugin of the service the user asks to act in", () => {
    const hits = run([{ kind: "user-message", text: "Create a Jira ticket for the login bug", at: 0 }]);
    expect(hits.map((hit) => [hit.momentId, hit.block])).toEqual([["service_without_plugin", { kind: "plugin", pluginId: "jira-client" }]]);
  });

  it("never offers an installed plugin, a mention without an action, or an unknown catalog", () => {
    expect(pluginForServiceRequest("Send this to Anya in Teams", FACTS.plugins ?? [])).toBeNull();
    expect(pluginForServiceRequest("What is Jira?", FACTS.plugins ?? [])).toBeNull();
    expect(run([{ kind: "user-message", text: "Create a Jira ticket", at: 0 }], { ...FACTS, plugins: null })).toEqual([]);
  });
});
