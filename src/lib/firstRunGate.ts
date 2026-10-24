import type { GeneralAgentConfig, Tab, Workspace } from "./types";
import type { AppShortcutHandlers } from "./appShortcuts";
import type { PageSelection } from "../store/multiplexer/pageSelection";
import { isFirstRunGeneralTab } from "../components/AgentPicker/constants";

export interface FirstRunGate {
  complete: boolean;
  allows(surface: "onboarding" | "settings" | "account" | "app"): boolean;
  allowsSettingsSection(section: string): boolean;
}

export function firstRunGate(general: Pick<GeneralAgentConfig, "first_run_complete"> | undefined): FirstRunGate {
  const complete = general?.first_run_complete === true;
  return { complete, allows: (surface) => complete || surface !== "app",
    allowsSettingsSection: (section) => complete || section !== "jobs" };
}

export function firstRunSelection(gate: FirstRunGate, state: PageSelection & {
  workspaces: readonly Workspace[];
  tabs: Readonly<Record<string, Tab>>;
}): PageSelection {
  if (gate.complete) return state;
  const workspace = state.workspaces.find((ws) => ws.tabs.some((id) => isFirstRunGeneralTab(state.tabs[id])));
  const tabId = workspace?.tabs.find((id) => isFirstRunGeneralTab(state.tabs[id])) ?? null;
  return { active_workspace_id: workspace?.id ?? null, focused_tab_id: tabId,
    activeTabIds: workspace && tabId ? { ...state.activeTabIds, [workspace.id]: tabId } : state.activeTabIds };
}

export function firstRunShortcutHandlers(gate: FirstRunGate, handlers: AppShortcutHandlers): AppShortcutHandlers {
  if (gate.allows("app")) return handlers;
  return Object.fromEntries(Object.entries(handlers).map(([action, handler]) => [
    action, action === "open_settings" ? handler : () => {},
  ])) as AppShortcutHandlers;
}

export function firstRunCommands<T extends { id: string }>(gate: FirstRunGate, commands: T[]): T[] {
  return gate.allows("app") ? commands : commands.filter((command) => command.id === "settings");
}
