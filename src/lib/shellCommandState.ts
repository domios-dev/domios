import type { Tab } from "./types";

export type ShellPurpose = "build" | "dev" | "qa" | "shell";
export interface ShellCommandState {
  activity: "unknown" | "running" | "idle";
  last_exit_code: number | null;
}

export function isUtilityShell(tab: Tab): boolean {
  return tab.tab_type === "Terminal" && tab.backing?.kind !== "chat_backend"
    && !tab.agent_type && !tab.role && !tab.roleless_tracked;
}

export function shellPurposeLabel(purpose: ShellPurpose = "shell"): string {
  return purpose === "shell" ? "shell" : `${purpose} shell`;
}

export function shellStateLabel(state?: ShellCommandState): string {
  if (!state || state.activity === "unknown") return "unknown";
  if (state.activity === "running") return "running";
  if (state.last_exit_code == null) return "idle";
  return state.last_exit_code === 0 ? "idle, last exit ok" : `idle, last exit failed (${state.last_exit_code})`;
}

export function applyShellState(tab: Tab, ptyId: string, state: ShellCommandState): Tab {
  if (!isUtilityShell(tab) || tab.pty_id !== ptyId) return tab;
  return { ...tab, shell_command_state: state };
}
