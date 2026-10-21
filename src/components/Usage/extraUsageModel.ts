import type {UsageDetails} from "../../types/usage";

type ExtraUsage = NonNullable<UsageDetails["extra_usage"]>;

function formatMinorAmount(amount: number, extra: ExtraUsage): string {
  const places = extra.decimal_places ?? 2;
  const decimals = Number.isInteger(places) && places >= 0 && places <= 20 ? places : 2;
  const major = amount / 10 ** decimals;
  try {
    return new Intl.NumberFormat("en-US", {style:"currency", currency:extra.currency, minimumFractionDigits:decimals, maximumFractionDigits:decimals}).format(major);
  } catch {
    return `${major.toFixed(decimals)} ${extra.currency}`;
  }
}

export function extraUsageLabel(extra: ExtraUsage): string {
  const parts = [`Usage credits ${extra.enabled ? "on" : "off"}`];
  if (extra.spend_limit_reached) parts.push("limit reached");
  else if (extra.disabled_reason) {
    parts.push(extra.disabled_reason === "out_of_credits" ? "no credits left" : extra.disabled_reason.replace(/[_-]+/g, " "));
  }
  if (extra.enabled) {
    if (extra.effective_enabled && extra.used != null) parts.push(`${formatMinorAmount(extra.used, extra)} used`);
    if (extra.limit != null) parts.push(`${formatMinorAmount(extra.limit, extra)}/mo limit`);
  }
  return parts.join(" · ");
}
