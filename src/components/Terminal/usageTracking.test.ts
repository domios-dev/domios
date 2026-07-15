import { usageTrackingEnabled, usageTrackingOverrides } from "./usageTracking";
import type { SettingField } from "../../lib/types";

const usageSection: SettingField[] = [{
  kind: "section",
  title: "Usage Tracking",
  fields: [{
    kind: "toggle",
    key: "usage_tracking_enabled",
    label: "Enable usage tracking",
    default: true,
  }],
}];

if (!usageTrackingEnabled(usageSection, undefined)) {
  throw new Error("usage-capable providers should honor the schema's enabled default");
}
if (usageTrackingEnabled(usageSection, false)) {
  throw new Error("an explicit false setting should disable usage tracking");
}
if (usageTrackingEnabled([], true)) {
  throw new Error("providers without the usage capability should not render usage UI");
}

const overrides = usageTrackingOverrides({
  defaultOn: {},
  enabled: { usage_tracking_enabled: true },
  disabled: { usage_tracking_enabled: false },
});
if (overrides.defaultOn !== undefined || overrides.enabled !== true || overrides.disabled !== false) {
  throw new Error("usage config mapping must preserve unset, enabled, and disabled states");
}

console.log("usageTracking tests: 4/4 passed");
