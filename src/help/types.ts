import type { KeybindingsConfig } from "../lib/types";

export type HelpSectionId =
  | "overview"
  | "workspaces"
  | "agents"
  | "plans"
  | "blueprints"
  | "memory"
  | "supporter-agent"
  | "mesh"
  | "messengers"
  | "phone"
  | "messenger-plugins"
  | "tips";

export type HelpFigureId =
  | "agent-hierarchy"
  | "plan-pipeline"
  | "task-status-flow"
  | "supporter-recall"
  | "supporter-tab-state";

export type HelpSettingsTarget = "supporter-agent" | "telegram-bridge";

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
  | { type: "heading"; text: string }
  | { type: "list"; items: string[]; ordered?: boolean }
  | { type: "figure"; figureId: HelpFigureId }
  | { type: "tip"; text: string }
  | { type: "ask"; label: string; question: string }
  | { type: "settings-link"; label: string; target: HelpSettingsTarget }
  | { type: "plugin"; pluginId: string }
  | { type: "connect"; bridgeId: string }
  | { type: "details"; summary: string; blocks: HelpBlock[] }
  | { type: "measurement"; label: string; text: string }
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
