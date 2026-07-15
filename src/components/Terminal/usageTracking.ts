import type { SettingField } from "../../lib/types";

function settingsContainKey(fields: SettingField[], key: string): boolean {
  return fields.some((field) => {
    if ("key" in field && field.key === key) return true;
    if (field.kind === "section") return settingsContainKey(field.fields, key);
    if (field.kind === "when") return settingsContainKey(field.then, key);
    return false;
  });
}

export function usageTrackingEnabled(
  settings: SettingField[],
  configured: boolean | undefined,
): boolean {
  return settingsContainKey(settings, "usage_tracking_enabled") && configured !== false;
}
