// Single source of truth for every "waiting" label in the UI. i18n-ready:
// keyed by locale like the help content module, so a translation is a new map.

export type BusyLabelKey =
  | "generic"
  | "tabBoot"
  | "chatPending"
  | "planLoad"
  | "planReconnect"
  | "memPending"
  | "meshPending"
  | "settingsPending"
  | "pickerPending"
  | "refreshing";

type BusyLabelMap = Record<BusyLabelKey, string>;

const en: BusyLabelMap = {
  generic: "Working",
  tabBoot: "Starting session",
  chatPending: "Thinking",
  planLoad: "Loading plan",
  planReconnect: "Reconnecting",
  memPending: "Loading memory",
  meshPending: "Contacting peer",
  settingsPending: "Loading",
  pickerPending: "Loading",
  refreshing: "Refreshing",
};

const LOCALES: Record<string, BusyLabelMap> = { en };

export function getBusyLabels(locale = "en"): BusyLabelMap {
  return LOCALES[locale] ?? en;
}

export function busyLabel(key: BusyLabelKey, locale = "en"): string {
  return getBusyLabels(locale)[key];
}
