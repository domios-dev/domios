import type { KeybindingsConfig } from "../lib/types";

export type HelpSectionId =
  | "overview"
  | "workspaces"
  | "agents"
  | "plans"
  | "memory"
  | "supporter-agent"
  | "mesh"
  | "integrations"
  | "tips";

export type HelpFigureId = "agent-hierarchy" | "plan-pipeline" | "task-status-flow";

export type HelpShortcutSource =
  | { kind: "config"; key: keyof KeybindingsConfig }
  | { kind: "app"; id: HelpAppShortcutId };

export type HelpAppShortcutId =
  | "command_palette"
  | "new_tab"
  | "close_tab"
  | "new_workspace"
  | "settings"
  | "toggle_sidebar";

export type HelpBlock =
  | { type: "paragraph"; text: string }
  | { type: "list"; items: string[] }
  | { type: "figure"; figureId: HelpFigureId }
  | { type: "tip"; text: string }
  | { type: "ask"; label: string; question: string }
  | {
      type: "shortcut-table";
      rows: Array<{ label: string; source: HelpShortcutSource }>;
    };

export interface HelpSectionContent {
  id: HelpSectionId;
  title: string;
  subtitle: string;
  blocks: HelpBlock[];
}

export interface HelpLocaleContent {
  locale: string;
  sections: HelpSectionContent[];
}
