import type { Tab } from "./types";
import { anchorsTeam } from "./roleKind";

export function isAgentCapableTab(tab: Tab): boolean {
  if (tab.agent_type) return true;
  if (tab.backing?.kind === "chat_backend") return true;
  return false;
}

export function canMakeOrchestrator(tab: Tab | null | undefined): boolean {
  if (!tab) return false;
  if (!isAgentCapableTab(tab)) return false;
  if (tab.protected) return false;
  if (anchorsTeam(tab)) return false;
  return true;
}
