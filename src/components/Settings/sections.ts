import {
  Sliders,
  Bell,
  Bot,
  KeyRound,
  FileText,
  Info,
  BookOpen,
  Workflow,
  type LucideIcon,
} from "lucide-react";
import type { HelpSettingsTarget } from "../../help/types";

export type SectionId =
  | "general"
  | "notifications"
  | "agents"
  | "secrets"
  | "jobs"
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
  secrets: KeyRound,
  jobs: Workflow,
  logs: FileText,
  guide: BookOpen,
  about: Info,
};

// The curated, user-facing nav list — order is the order shown in the menu and panel.
export const SETTINGS_SECTIONS: SettingsSection[] = [
  { id: "general", label: "General", desc: "Theme, fonts, messengers, keybindings", icon: SECTION_ICONS.general },
  { id: "notifications", label: "Notifications", desc: "The bell, desktop alerts, sound, and where each level may go", icon: SECTION_ICONS.notifications },
  { id: "agents", label: "Agents", desc: "General Agent, agent access, and per-provider config", icon: SECTION_ICONS.agents },
  { id: "jobs", label: "Jobs", desc: "Crons and Blueprints on this machine", icon: SECTION_ICONS.jobs },
  { id: "logs", label: "Logs", desc: "App diagnostics — live, filterable, scoped", icon: SECTION_ICONS.logs },
  { id: "guide", label: "Guide", desc: "What Domios is and how to work with it", icon: SECTION_ICONS.guide },
  { id: "about", label: "Check for updates", desc: "Version, updates, creator", icon: SECTION_ICONS.about },
];

export function sectionTitle(id: SectionId): string {
  return SETTINGS_SECTIONS.find((s) => s.id === id)?.label ?? id;
}

export type SettingsCardAnchorId = "supporter-agent" | "telegram-bridge";

export interface SettingsCardRoute {
  section: SectionId;
  card: SettingsCardAnchorId;
}

/** Retired section ids that still arrive from old links. */
export type LegacySectionId = "integrations";
export type SettingsLinkId = SectionId | LegacySectionId;

export interface SettingsLinkRoute {
  section: SectionId;
  card?: SettingsCardAnchorId;
}

const LEGACY_ROUTES: Record<LegacySectionId, SettingsLinkRoute> = {
  integrations: { section: "general", card: "telegram-bridge" },
};

export function resolveSettingsLink(id: SettingsLinkId | undefined): SettingsLinkRoute | undefined {
  if (!id) return undefined;
  return id in LEGACY_ROUTES ? LEGACY_ROUTES[id as LegacySectionId] : { section: id as SectionId };
}

const HELP_SETTINGS_ROUTES: Record<HelpSettingsTarget, SettingsCardRoute> = {
  "supporter-agent": { section: "agents", card: "supporter-agent" },
};

export function settingsRouteForHelpTarget(target: HelpSettingsTarget): SettingsCardRoute {
  return HELP_SETTINGS_ROUTES[target];
}
