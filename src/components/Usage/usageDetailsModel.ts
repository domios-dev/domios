export function usageRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function usageField(value: unknown, key: string): unknown {
  return usageRecord(value) ? value[key] : undefined;
}

const finite = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);
const nullableNumber = (value: unknown): value is number | null => value === null || finite(value);

export function usageLimits(details: unknown) {
  const limits = usageField(details, "limits");
  return Array.isArray(limits) ? limits.flatMap((limit: unknown) => {
    if (!usageRecord(limit) || typeof limit.kind !== "string" || typeof limit.label !== "string"
      || !finite(limit.percent) || !nullableNumber(limit.resets_at)) return [];
    return [{ kind: limit.kind, label: limit.label, percent: limit.percent, resets_at: limit.resets_at,
      is_active: limit.is_active === true, critical: limit.critical === true }];
  }) : [];
}

export function dollarCredits(details: unknown) {
  const credits = usageField(details, "dollar_credits");
  return Array.isArray(credits) ? credits.flatMap((credit: unknown) => {
    if (!usageRecord(credit) || !finite(credit.remaining_dollars) || !nullableNumber(credit.resets_at)) return [];
    return [{ remaining_dollars: credit.remaining_dollars, resets_at: credit.resets_at }];
  }) : [];
}

export function extraUsage(value: unknown) {
  if (!usageRecord(value) || typeof value.enabled !== "boolean" || typeof value.spend_limit_reached !== "boolean"
    || !nullableNumber(value.used) || !nullableNumber(value.limit) || typeof value.currency !== "string"
    || typeof value.settings_url !== "string") return null;
  return { enabled: value.enabled, spend_limit_reached: value.spend_limit_reached,
    used: value.used, limit: value.limit, currency: value.currency, settings_url: value.settings_url,
    effective_enabled: typeof value.effective_enabled === "boolean" ? value.effective_enabled : undefined,
    disabled_reason: typeof value.disabled_reason === "string" ? value.disabled_reason : null,
    decimal_places: finite(value.decimal_places) ? value.decimal_places : undefined };
}
