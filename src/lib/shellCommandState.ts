import { isPtyBacked } from "./surfaceTab";
import type { Tab } from "./types";

export const SHELL_PURPOSES = ["build", "dev", "qa", "deploy", "logs", "db", "shell"] as const;
export type ShellPurpose = (typeof SHELL_PURPOSES)[number];

export function isShellPurpose(value: string | undefined): value is ShellPurpose {
  return SHELL_PURPOSES.includes(value as ShellPurpose);
}
export interface ShellCommandState {
  activity: "unknown" | "running" | "idle";
  last_exit_code: number | null;
}

export function isUtilityShell(tab: Tab): boolean {
  return tab.tab_type === "Terminal" && isPtyBacked(tab)
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
