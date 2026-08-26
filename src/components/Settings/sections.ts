import {
  Sliders,
  Bell,
  Bot,
  Plug,
  KeyRound,
  FileText,
  Info,
  BookOpen,
  type LucideIcon,
} from "lucide-react";

export type SectionId =
  | "general"
  | "notifications"
  | "agents"
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

// (secrets lives in the Extensions surface but still needs a breadcrumb icon).
export const SECTION_ICONS: Record<SectionId, LucideIcon> = {
  general: Sliders,
  notifications: Bell,
  agents: Bot,
  integrations: Plug,
  secrets: KeyRound,
  logs: FileText,
  guide: BookOpen,
  about: Info,
};

// The curated, user-facing nav list — order is the order shown in the menu and panel.
export const SETTINGS_SECTIONS: SettingsSection[] = [
  { id: "general", label: "General", desc: "Theme, fonts, cursor, keybindings", icon: SECTION_ICONS.general },
  { id: "notifications", label: "Notifications", desc: "The bell, desktop alerts, sound, and where each level may go", icon: SECTION_ICONS.notifications },
  { id: "agents", label: "Agents", desc: "General Agent, agent access, and per-provider config", icon: SECTION_ICONS.agents },
  { id: "integrations", label: "Integrations", desc: "Telegram, Discord, external bridges", icon: SECTION_ICONS.integrations },
  { id: "logs", label: "Logs", desc: "App diagnostics — live, filterable, scoped", icon: SECTION_ICONS.logs },
  { id: "guide", label: "Guide", desc: "What CodeTerm is and how to work with it", icon: SECTION_ICONS.guide },
  { id: "about", label: "Check for updates", desc: "Version, updates, creator", icon: SECTION_ICONS.about },
];

export function sectionTitle(id: SectionId): string {
  return SETTINGS_SECTIONS.find((s) => s.id === id)?.label ?? id;
}

// "notifications" was a deep-link alias for Agents; the section is real now, so the link resolves to itself.
export function normalizeSection(id: SectionId | undefined): SectionId | undefined {
  return id;
}
