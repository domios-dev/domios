import { enHelpContent } from "./en";
import type { HelpLocaleContent } from "../types";

const LOCALES: Record<string, HelpLocaleContent> = {
  en: enHelpContent,
};

export function getHelpContent(locale = "en"): HelpLocaleContent {
  return LOCALES[locale] ?? enHelpContent;
}

export function getHelpSectionIds(locale = "en"): string[] {
  return getHelpContent(locale).sections.map((s) => s.id);
}
