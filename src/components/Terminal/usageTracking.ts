import type { RuntimeProfileLimitsSource } from "@codeterm/plugin-sdk";
import type { ProviderConfig, SettingField } from "../../lib/types";

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

export function usageTrackingOverrides(
  agents: Record<string, ProviderConfig | undefined>,
): Record<string, boolean | undefined> {
  const out: Record<string, boolean | undefined> = {};
  for (const [providerId, config] of Object.entries(agents)) {
    out[providerId] = config?.usage_tracking_enabled ?? undefined;
  }
  return out;
}

export function tabUsageProvider(provider:string | null | undefined, chatBackend:boolean, runtimeUsage:boolean | undefined): string | null {
  return !chatBackend || runtimeUsage ? provider ?? null : null;
}

/** What a tab's usage bubble shows. `trackingProvider` owns the Usage Tracking toggle that gates it. */
export type TabUsageSource =
  | {kind:"meter"; provider:string; trackingProvider:string; borrowedAccount:boolean}
  | {kind:"balance" | "quota"; provider:string; accountId:string; trackingProvider:string}
  | {kind:"unavailable"; reason:string; trackingProvider:string};

export interface TabLaunchProfile {id:string; limitsSource?:RuntimeProfileLimitsSource}

/** A chat-backend tab follows its launch profile's declared limits source; the kind alone decides. */
export function tabUsageSource(provider:string | null | undefined, chatBackend:boolean, runtimeUsage:boolean | undefined, profile?:TabLaunchProfile): TabUsageSource | null {
  const source = chatBackend ? profile?.limitsSource : undefined;
  switch (source?.kind) {
    case "account": return {kind:"meter", provider:source.provider, trackingProvider:source.provider, borrowedAccount:true};
    case "balance":
    case "quota": return provider && profile ? {kind:source.kind, provider, accountId:`profile:${profile.id}`, trackingProvider:provider} : null;
    case "unavailable": return provider ? {kind:"unavailable", reason:source.reason, trackingProvider:provider} : null;
  }
  const meter = tabUsageProvider(provider, chatBackend, runtimeUsage);
  return meter ? {kind:"meter", provider:meter, trackingProvider:meter, borrowedAccount:false} : null;
}
