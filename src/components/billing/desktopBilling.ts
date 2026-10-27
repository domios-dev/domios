import { parsePlanLimits, type PlanCatalog, type PlanOffer, type PlanPrice, type BillingInterval } from "./planCatalog";
import { localFetch } from "../../lib/webApi";

export interface SubscriptionReadback {
  current_tier: string;
  billing_interval: BillingInterval | null;
  period_end: number | null;
  pending_change: { to_tier: string; effective_at: number; status: "scheduled" | "confirmation_pending" } | null;
}

const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 128;
const seconds = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const TIER_NAMES: Readonly<Record<string, string>> = { free: "Free", pro: "Pro", max: "Max", sponsor: "Sponsor" };
export const tierName = (tier: string): string => TIER_NAMES[tier] ?? tier;
const money = (value: unknown): value is string => typeof value === "string" && /^\d{1,14}$/.test(value);

function price(value: unknown, tier: string, interval: "monthly" | "yearly"): PlanPrice | null {
  if (value === null) return null;
  if (!record(value) || !text(value.price_id) || !money(value.amount) || !money(value.list_price) || !money(value.discount)
    || typeof value.currency_code !== "string" || !/^[A-Z]{3}$/.test(value.currency_code)) throw new Error("Invalid catalog price");
  const decimals = new Intl.NumberFormat("en-US", { style: "currency", currency: value.currency_code }).resolvedOptions().maximumFractionDigits ?? 2;
  return { priceId: value.price_id, planKey: `${tier}_${interval}`, amount: Number(value.amount) / 10 ** decimals, currency: value.currency_code };
}

export function parseDesktopCatalog(value: unknown): PlanCatalog {
  if (!record(value) || value.status !== "available" || !record(value.catalog)) throw new Error("Plan catalog unavailable");
  const catalog = value.catalog;
  if (catalog.schema_version !== 1 || !["test", "live"].includes(String(catalog.mode)) || !Array.isArray(catalog.tiers)
    || catalog.tiers.length === 0 || catalog.tiers.length > 32) throw new Error("Invalid plan catalog");
  const offers = catalog.tiers.map((tier): PlanOffer => {
    if (!record(tier) || !text(tier.tier) || !record(tier.limits) || !record(tier.prices)) throw new Error("Invalid tier");
    const limits = parsePlanLimits({ "mesh.machines": tier.limits.mesh_machines, workspaces: tier.limits.workspaces,
      "mem.items": tier.limits.mem_items, teams: tier.limits.teams, "agent.tabs": tier.limits.agent_tabs, neighbors: tier.limits.neighbors });
    const month = price(tier.prices.monthly, tier.tier, "monthly");
    const year = price(tier.prices.yearly, tier.tier, "yearly");
    const trial = catalog.trial_offer;
    if (month && record(trial) && trial.tier === tier.tier && trial.price_id === month.priceId
      && seconds(trial.days) && trial.days > 0 && typeof trial.eligible === "boolean") {
      month.trialDays = trial.days;
      month.trialEligible = trial.eligible;
    }
    return { id: tier.tier, name: tierName(tier.tier), tagline: "", features: [], limits,
      prices: { ...(month ? { month } : {}), ...(year ? { year } : {}) }, ...(tier.tier === "pro" ? { recommended: true } : {}) };
  });
  if (new Set(offers.map(offer => offer.id)).size !== offers.length) throw new Error("Duplicate catalog tier");
  return { offers, changesAvailable: true };
}

export function parseSubscription(value: unknown): SubscriptionReadback {
  if (!record(value) || !text(value.current_tier) || (value.period_end !== null && !seconds(value.period_end))) throw new Error("Invalid subscription response");
  const interval = value.billing_interval ?? null;
  if (interval !== null && interval !== "month" && interval !== "year") throw new Error("Invalid subscription interval");
  const pending = value.pending_change;
  let pendingChange: SubscriptionReadback["pending_change"] = null;
  if (pending !== null) {
    if (!record(pending) || !text(pending.to_tier) || !seconds(pending.effective_at)
      || (pending.status !== "scheduled" && pending.status !== "confirmation_pending")) throw new Error("Invalid pending plan change");
    pendingChange = { to_tier: pending.to_tier, effective_at: pending.effective_at, status: pending.status };
  }
  return { current_tier: value.current_tier, billing_interval: interval, period_end: value.period_end, pending_change: pendingChange };
}

export function scheduledPlanCopy(value: SubscriptionReadback, formatDate: (seconds: number) => string): string | null {
  const pending = value.pending_change;
  if (!pending) return null;
  if (pending.status === "confirmation_pending") return "Your plan change is awaiting confirmation. Please do not submit it again.";
  const date = formatDate(pending.effective_at);
  return `${tierName(value.current_tier)} until ${date}, ${tierName(pending.to_tier)} from ${date}`;
}

export async function billingOperation(operation: "catalog" | "subscription" | "change", priceId?: string, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/billing", { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ operation, ...(priceId ? { price_id: priceId } : {}) }), signal });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}
