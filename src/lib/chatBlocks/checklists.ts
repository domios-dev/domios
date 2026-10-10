import type { ChecklistInfo, ChecklistItemAction } from "@codeterm/chat-ui";
import type { CatalogEntry } from "../../components/Extensions/pluginCatalog";
import { isGeneralAgent } from "../roleKind";
import type { PushChannelDescriptor } from "../notifyDestinations";
import type { Tab } from "../types";
import type { JobsSnapshot } from "./jobsBlockStore";

/** Real-state facts a checklist ticks from; null while a fact is still unknown. */
export interface SetupSignals {
  agentSpawned: boolean | null;
  phoneConnected: boolean | null;
  pluginInstalled: boolean | null;
  blueprintCreated: boolean | null;
  memorySaved: boolean | null;
}

export type SetupLink = "new-agent" | "plugins" | "jobs" | "memory";

export interface ChecklistItemDefinition {
  id: string;
  label: string;
  hint: string;
  signal: keyof SetupSignals;
  action: { kind: "block"; label: string; source: string } | { kind: "link"; label: string; link: SetupLink };
}

export interface ChecklistDefinition {
  id: string;
  title: string;
  items: readonly ChecklistItemDefinition[];
}

export const GETTING_STARTED: ChecklistDefinition = {
  id: "getting-started",
  title: "Getting started",
  items: [
    {
      id: "first-agent",
      label: "Start your first agent",
      hint: "Open a new agent tab with the provider and model you want.",
      signal: "agentSpawned",
      action: { kind: "link", label: "New agent", link: "new-agent" },
    },
    {
      id: "phone",
      label: "Connect your phone",
      hint: "Answer agents from a Telegram bot while you are away.",
      signal: "phoneConnected",
      action: { kind: "block", label: "Show how", source: "@ds-guide phone" },
    },
    {
      id: "plugin",
      label: "Install a plugin",
      hint: "Plugins let agents use services such as your Telegram or Teams account.",
      signal: "pluginInstalled",
      action: { kind: "link", label: "Browse plugins", link: "plugins" },
    },
    {
      id: "blueprint",
      label: "Create a Blueprint",
      hint: "Ask an agent to automate a recurring job; it appears in Jobs.",
      signal: "blueprintCreated",
      action: { kind: "link", label: "Open Jobs", link: "jobs" },
    },
    {
      id: "memory",
      label: "Save a memory",
      hint: "Tell an agent “remember …” and every later session can recall it.",
      signal: "memorySaved",
      action: { kind: "link", label: "Open Memory", link: "memory" },
    },
  ],
};

export const CHECKLISTS: Readonly<Record<string, ChecklistDefinition>> = { [GETTING_STARTED.id]: GETTING_STARTED };

/** Kinds a person or agent saves on purpose; seeded skills, dreams and transcripts do not count. */
const SAVED_MEMORY_KINDS = ["fact", "learning", "note", "preference", "rule"] as const;

export function agentSpawned(tabs: Readonly<Record<string, Tab>>): boolean {
  return Object.values(tabs).some((tab) => Boolean(tab.agent_type) && !tab.hidden && !isGeneralAgent(tab));
}

export function phoneConnected(channels: readonly PushChannelDescriptor[] | null): boolean | null {
  return channels === null ? null : channels.some((channel) => channel.control_available);
}

export function pluginInstalled(loaded: boolean, catalog: readonly CatalogEntry[]): boolean | null {
  if (catalog.some((entry) => entry.installed && !entry.system && !entry.dev)) return true;
  return loaded ? false : null;
}

export function blueprintCreated(jobs: JobsSnapshot): boolean | null {
  if (jobs.graphs?.some(({ graph }) => (graph.blueprints?.length ?? 0) > 0)) return true;
  return jobs.graphs === null ? null : false;
}

export function memorySaved(byKind: Readonly<Record<string, number>> | null): boolean | null {
  return byKind === null ? null : SAVED_MEMORY_KINDS.some((kind) => (byKind[kind] ?? 0) > 0);
}

export function checklistInfo(definition: ChecklistDefinition, signals: SetupSignals): ChecklistInfo {
  return {
    id: definition.id,
    title: definition.title,
    items: definition.items.map((item) => ({
      id: item.id,
      label: item.label,
      hint: item.hint,
      done: signals[item.signal],
      action: (item.action.kind === "block"
        ? { kind: "block", label: item.action.label, source: item.action.source }
        : { kind: "link", label: item.action.label }) satisfies ChecklistItemAction,
    })),
  };
}
