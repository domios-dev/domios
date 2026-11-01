import { useEffect, useRef } from "react";
import { recordProductEvent } from "../lib/productAnalytics";
import type { HelpSectionId } from "../help/types";
import { GUIDE_SECTIONS, type GuideRequest } from "../help/guideNavigation";
import { SECTION_ICONS, resolveSettingsLink, type SectionId, type SettingsLinkId, type SettingsCardAnchorId } from "../components/Settings/sections";
import { navigationId, navigationObject } from "../lib/viewerNavigation";
import { useViewerNavigation } from "./useViewerNavigation";

export type SettingsRoute =
  | { kind: "root" }
  | { kind: "section"; id: SectionId; helpSection?: HelpSectionId; card?: SettingsCardAnchorId }
  | { kind: "agent-provider"; providerId: string };

function initialStack(link: SettingsLinkId | undefined, guide: GuideRequest | undefined): SettingsRoute[] {
  const route = resolveSettingsLink(link);
  return route ? [{ kind: "root" }, { kind: "section", id: route.section, card: route.card, helpSection: route.section === "guide" ? guide?.section : undefined }] : [{ kind: "root" }];
}

export function settingsStack(value: unknown): SettingsRoute[] | undefined {
  if (!Array.isArray(value) || value.length > 4) return undefined;
  const stack: SettingsRoute[] = [{ kind: "root" }];
  for (const entry of value.slice(1)) {
    const route = navigationObject(entry);
    if (route?.kind === "section" && typeof route.id === "string" && Object.prototype.hasOwnProperty.call(SECTION_ICONS, route.id)) {
      const card = route.card === "supporter-agent" || route.card === "telegram-bridge" || route.card === "subscription-plans" ? route.card : undefined;
      const helpSection = GUIDE_SECTIONS.find((section) => section.id === route.helpSection)?.id;
      stack.push({ kind: "section", id: route.id as SectionId, card, helpSection });
    } else if (route?.kind === "agent-provider" && navigationId(route.providerId)) stack.push({ kind: "agent-provider", providerId: route.providerId });
    else break;
  }
  return stack;
}

export function useSettingsNavigation(link: SettingsLinkId | undefined, guide: GuideRequest | undefined) {
  const [stack, setStack] = useViewerNavigation<SettingsRoute[]>("settings", () => initialStack(link, guide), settingsStack, link === undefined);
  const previous = useRef({ link, guide });
  const route = stack[stack.length - 1];
  const section = route.kind === "section" ? route.id : route.kind === "agent-provider" ? "agent_provider" : "root";
  const viewedSection = useRef<string | null>(null);
  useEffect(() => {
    if (viewedSection.current === section) return;
    viewedSection.current = section;
    void recordProductEvent({ event_type: "settings_viewed", event_properties: { section } });
  }, [section]);
  useEffect(() => {
    if (previous.current.link !== link || previous.current.guide !== guide) setStack(initialStack(link, guide));
    previous.current = { link, guide };
  }, [link, guide, setStack]);
  return [stack, setStack] as const;
}
