import { getHelpContent } from "./content";
import type { HelpSectionId } from "./types";

export interface GuideRequest {
  section: HelpSectionId;
}

export const GUIDE_SECTIONS = getHelpContent().sections.map(({ id, title }) => ({ id, title }));

export function guideRequest(sectionId: string): GuideRequest {
  return {
    section: GUIDE_SECTIONS.find((section) => section.id === sectionId)?.id ?? GUIDE_SECTIONS[0].id,
  };
}
