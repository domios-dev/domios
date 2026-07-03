import {
  Sliders,
  Bot,
  Package,
  Plug,
  KeyRound,
  FileText,
  Info,
  BookOpen,
  type LucideIcon,
} from "lucide-react";

export type SectionId =
  | "general"
  | "agents"
  | "plugins"
  | "integrations"
  | "secrets"
  | "logs"
  | "guide"
  | "about";

export interface SettingsSection {
  id: SectionId;
  label: string;
  desc: string;
  icon: LucideIcon;
}

// Icon for every section id — including ones not surfaced in the nav list
// (plugins/secrets live in the Extensions surface but still need breadcrumb icons).
export const SECTION_ICONS: Record<SectionId, LucideIcon> = {
  general: Sliders,
  agents: Bot,
  plugins: Package,
  integrations: Plug,
  secrets: KeyRound,
  logs: FileText,
  guide: BookOpen,
  about: Info,
};

// The curated, user-facing nav list — order is the order shown in the menu and panel.
export const SETTINGS_SECTIONS: SettingsSection[] = [
  { id: "general", label: "General", desc: "Theme, fonts, cursor, keybindings", icon: SECTION_ICONS.general },
  { id: "agents", label: "Agents", desc: "General Agent, notifications, and per-provider config", icon: SECTION_ICONS.agents },
  { id: "integrations", label: "Integrations", desc: "Telegram, Discord, external bridges", icon: SECTION_ICONS.integrations },
  { id: "logs", label: "Logs", desc: "App diagnostics — live, filterable, scoped", icon: SECTION_ICONS.logs },
  { id: "guide", label: "Guide", desc: "What CodeTerm is and how to work with it", icon: SECTION_ICONS.guide },
  { id: "about", label: "Check for updates", desc: "Version, updates, creator", icon: SECTION_ICONS.about },
];

export function sectionTitle(id: SectionId): string {
  return SETTINGS_SECTIONS.find((s) => s.id === id)?.label ?? id;
}

// "notifications" is a legacy deep-link alias that resolves to the Agents section.
export function normalizeSection(id: SectionId | "notifications" | undefined): SectionId | undefined {
  if (!id) return undefined;
  return id === "notifications" ? "agents" : id;
}
