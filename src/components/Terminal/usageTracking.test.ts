import { usageTrackingEnabled, usageTrackingOverrides, tabUsageProvider, tabUsageSource } from "./usageTracking";
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

if (tabUsageProvider("fixture",true,true) !== "fixture" || tabUsageProvider("fixture",true,false) !== null || tabUsageProvider("fixture",false,false) !== "fixture") {
  throw new Error("headers select runtime usage by capability and retain terminal usage");
}

const sourceCases: [Parameters<typeof tabUsageSource>, ReturnType<typeof tabUsageSource>][] = [
  [["fixture-acp", true, true, {id:"borrowing", limitsSource:{kind:"account", provider:"fixture-native"}}],
    {kind:"meter", provider:"fixture-native", trackingProvider:"fixture-native", borrowedAccount:true}],
  [["fixture-acp", true, false, {id:"balanced", limitsSource:{kind:"balance", reader:"fixture-balance", credentialEnv:"KEY"}}],
    {kind:"balance", provider:"fixture-acp", accountId:"profile:balanced", trackingProvider:"fixture-acp"}],
  [["fixture-acp", true, false, {id:"quoted", limitsSource:{kind:"quota", reader:"fixture-quota"}}],
    {kind:"quota", provider:"fixture-acp", accountId:"profile:quoted", trackingProvider:"fixture-acp"}],
  [["fixture-acp", true, true, {id:"opaque", limitsSource:{kind:"unavailable", reason:"Opaque reason"}}],
    {kind:"unavailable", reason:"Opaque reason", trackingProvider:"fixture-acp"}],
  [["fixture-acp", true, true, {id:"undeclared"}], {kind:"meter", provider:"fixture-acp", trackingProvider:"fixture-acp", borrowedAccount:false}],
  [["fixture-acp", true, false, {id:"undeclared"}], null],
  [["fixture-pty", false, false, {id:"ignored", limitsSource:{kind:"unavailable", reason:"Opaque reason"}}],
    {kind:"meter", provider:"fixture-pty", trackingProvider:"fixture-pty", borrowedAccount:false}],
];
for (const [args, expected] of sourceCases) {
  if (JSON.stringify(tabUsageSource(...args)) !== JSON.stringify(expected)) {
    throw new Error(`usage source follows only the declared kind: ${JSON.stringify(args)}`);
  }
}
console.log("tabUsageSource: declared kind selects account, balance, quota or unavailable; undeclared keeps the runtime meter");
