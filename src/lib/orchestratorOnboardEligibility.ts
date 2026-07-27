import type { Pane } from "./types";

export function isAgentCapablePane(pane: Pane): boolean {
  if (pane.agent_type) return true;
  if (pane.backing?.kind === "chat_backend") return true;
  return false;
}

export function canMakeOrchestrator(pane: Pane | null | undefined): boolean {
  if (!pane) return false;
  if (!isAgentCapablePane(pane)) return false;
  if (pane.protected) return false;
  if (pane.role === "Orchestrator") return false;
  return true;
}
