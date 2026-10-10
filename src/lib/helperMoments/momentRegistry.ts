import type { TabStatus } from "../types";
import type { HelperBlock, HelperFacts, HelperPluginFact } from "./helperBlocks";

export type MomentId =
  | "waiting_on_user_while_away"
  | "service_without_plugin"
  | "long_task_finished_unattended"
  | "repeated_manual_action"
  | "adjacent_capability";

export type HelperTabKind = "agent" | "manual-shell" | "other";

/** Existing host events, reduced to what the moments read. */
export type HelperSignal =
  | { kind: "window-focus"; focused: boolean; at: number }
  | { kind: "tab-status"; tabId: string; status: TabStatus; at: number }
  | { kind: "shell-command"; tabId: string; commandLine: string; at: number }
  | { kind: "user-message"; text: string; at: number };

export interface TrackedTab {
  status: TabStatus;
  since: number;
  workingSince: number | null;
}

/** Observable history the conditions need, and nothing else. */
export interface MomentTracker {
  blurredAt: number | null;
  tabs: Readonly<Record<string, TrackedTab>>;
  commands: Readonly<Record<string, readonly string[]>>;
}

export const EMPTY_TRACKER: MomentTracker = { blurredAt: null, tabs: {}, commands: {} };

export const MOMENT_THRESHOLDS = {
  awayMs: 3 * 60_000,
  longRunMs: 10 * 60_000,
  repeatCount: 3,
  minSequence: 2,
  maxSequence: 3,
  historyLength: 24,
};

export interface MomentContext {
  signal: HelperSignal;
  before: MomentTracker;
  after: MomentTracker;
  facts: HelperFacts;
}

export interface MomentHit {
  momentId: MomentId;
  block: HelperBlock;
  benefit: string;
  tabId?: string;
}

export interface MomentDefinition {
  id: Exclude<MomentId, "adjacent_capability">;
  detect(context: MomentContext): { block: HelperBlock; benefit: string; tabId?: string } | null;
}

const PHONE: HelperBlock = { kind: "guide", sectionId: "phone" };
const BLUEPRINTS: HelperBlock = { kind: "guide", sectionId: "blueprints" };

function awayOverlap(blurredAt: number | null, since: number, at: number): number {
  return blurredAt === null ? 0 : at - Math.max(blurredAt, since);
}

/** Registry order is priority: one signal yields at most the first matching moment. */
export const MOMENTS: readonly MomentDefinition[] = [
  {
    id: "waiting_on_user_while_away",
    detect: ({ signal, before }) => {
      const waitedAway = (tab: TrackedTab | undefined) =>
        tab?.status === "Waiting" && awayOverlap(before.blurredAt, tab.since, signal.at) >= MOMENT_THRESHOLDS.awayMs;
      const hit = signal.kind === "window-focus" && signal.focused
        ? Object.values(before.tabs).some(waitedAway)
        : signal.kind === "tab-status" && signal.status !== "Waiting" && waitedAway(before.tabs[signal.tabId]);
      return hit
        ? { block: PHONE, benefit: "An agent waited for your answer while you were away. Connect your phone and you can answer it from anywhere." }
        : null;
    },
  },
  {
    id: "service_without_plugin",
    detect: ({ signal, facts }) => {
      if (signal.kind !== "user-message" || !facts.plugins) return null;
      const plugin = pluginForServiceRequest(signal.text, facts.plugins);
      return plugin
        ? { block: { kind: "plugin", pluginId: plugin.id }, benefit: `${plugin.name} is not installed yet. Install it and agents can work there for you.` }
        : null;
    },
  },
  {
    id: "long_task_finished_unattended",
    detect: ({ signal, before }) => {
      if (signal.kind !== "tab-status" || signal.status !== "Idle" || before.blurredAt === null) return null;
      const workingSince = before.tabs[signal.tabId]?.workingSince;
      if (workingSince == null || signal.at - workingSince < MOMENT_THRESHOLDS.longRunMs) return null;
      return { block: PHONE, benefit: "A long run finished while you were away. Connect your phone and results like this reach you as they land.", tabId: signal.tabId };
    },
  },
  {
    id: "repeated_manual_action",
    detect: ({ signal, after }) => {
      if (signal.kind !== "shell-command") return null;
      return repeatedSequence(after.commands[signal.tabId] ?? [])
        ? { block: BLUEPRINTS, benefit: "You have run the same commands here several times. A Blueprint can watch for the change and run them for you.", tabId: signal.tabId }
        : null;
    },
  },
];

function nextTab(previous: TrackedTab | undefined, status: TabStatus, at: number): TrackedTab {
  const same = previous?.status === status;
  const working = status === "Working";
  return {
    status,
    since: same && previous ? previous.since : at,
    workingSince: working ? (previous?.status === "Working" ? previous.workingSince : at) : null,
  };
}

function omit<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  const { [key]: _removed, ...rest } = record;
  return rest;
}

/** Folds one signal into the tracker; tabs the caller does not classify as agent or manual shell are ignored. */
export function trackSignal(tracker: MomentTracker, signal: HelperSignal, tabKind: (tabId: string) => HelperTabKind): MomentTracker {
  switch (signal.kind) {
    case "window-focus":
      if (signal.focused) return tracker.blurredAt === null ? tracker : { ...tracker, blurredAt: null };
      return tracker.blurredAt === null ? { ...tracker, blurredAt: signal.at } : tracker;
    case "tab-status":
      if (signal.status === "Dead") return { ...tracker, tabs: omit(tracker.tabs, signal.tabId), commands: omit(tracker.commands, signal.tabId) };
      if (tabKind(signal.tabId) !== "agent") return tracker;
      return { ...tracker, tabs: { ...tracker.tabs, [signal.tabId]: nextTab(tracker.tabs[signal.tabId], signal.status, signal.at) } };
    case "shell-command": {
      const command = normalizeCommand(signal.commandLine);
      if (!command || tabKind(signal.tabId) !== "manual-shell") return tracker;
      const history = [...(tracker.commands[signal.tabId] ?? []), command].slice(-MOMENT_THRESHOLDS.historyLength);
      return { ...tracker, commands: { ...tracker.commands, [signal.tabId]: history } };
    }
    case "user-message":
      return tracker;
  }
}

/** Tracks the signal and returns the first moment it completes, if any. */
export function observeSignal(
  tracker: MomentTracker,
  signal: HelperSignal,
  facts: HelperFacts,
  tabKind: (tabId: string) => HelperTabKind,
): { tracker: MomentTracker; hit: MomentHit | null } {
  const after = trackSignal(tracker, signal, tabKind);
  for (const moment of MOMENTS) {
    const found = moment.detect({ signal, before: tracker, after, facts });
    if (found) return { tracker: after, hit: { momentId: moment.id, ...found } };
  }
  return { tracker: after, hit: null };
}

export function normalizeCommand(commandLine: string): string {
  return commandLine.trim().replace(/\s+/g, " ");
}

const TRIVIAL_PROGRAMS = new Set(["ls", "ll", "dir", "cd", "pwd", "clear", "cls", "exit", "history", "echo"]);
const TRIVIAL_COMMANDS = new Set(["git status", "git diff", "git log"]);

export function isTrivialCommand(command: string): boolean {
  return TRIVIAL_COMMANDS.has(command) || TRIVIAL_PROGRAMS.has(command.split(" ")[0].toLowerCase());
}

/** True when the newest sequence of two or more distinct commands recurs at least `repeatCount` times. */
export function repeatedSequence(history: readonly string[]): boolean {
  for (let length = MOMENT_THRESHOLDS.minSequence; length <= MOMENT_THRESHOLDS.maxSequence && length <= history.length; length++) {
    const tail = history.slice(-length);
    if (tail.every(isTrivialCommand) || new Set(tail).size < 2) continue;
    let count = 0;
    let index = history.length - length;
    while (index >= 0) {
      if (tail.every((command, offset) => history[index + offset] === command)) {
        count++;
        index -= length;
      } else {
        index--;
      }
    }
    if (count >= MOMENT_THRESHOLDS.repeatCount) return true;
  }
  return false;
}

const GENERIC_TERMS = new Set(["client", "plugin", "bridge", "connector", "integration", "domios", "codeterm", "agent", "tools", "server", "official"]);
const ACTION = /\b(send|post|message|reply|write|read|check|find|search|open|create|add|update|delete|schedule|book|upload|share|forward|fetch|list|show|summari[sz]e|draft|invite|sync|import|export)\b|отправ|напиш|написа|прочит|провер|найд|найти|созда|добав|обнов|удали|пошли|скинь|покажи|ответь/i;

/** Distinctive words naming the service a plugin works in, from its id and display name. */
export function pluginServiceTerms(plugin: Pick<HelperPluginFact, "id" | "name">): string[] {
  const words = [...plugin.id.split(/[-._]/), ...plugin.name.split(/\s+/)].map((word) => word.toLowerCase().replace(/[^a-z0-9]/g, ""));
  return [...new Set(words.filter((word) => word.length >= 4 && !GENERIC_TERMS.has(word)))];
}

/** The first not-installed plugin whose service the message asks to act in, or null. */
export function pluginForServiceRequest(text: string, plugins: readonly HelperPluginFact[]): HelperPluginFact | null {
  if (!ACTION.test(text)) return null;
  const lower = text.toLowerCase();
  return plugins.find((plugin) => !plugin.installed
    && pluginServiceTerms(plugin).some((term) => new RegExp(`(^|[^a-z0-9])${term}([^a-z0-9]|$)`).test(lower))) ?? null;
}
