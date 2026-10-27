import { TAB_LIMIT_NOUN } from "./limitCopy";

export type BillingInterval = "month" | "year";

export interface PlanPrice {
  priceId?: string;
  /** The provider's checkout key for this price, e.g. a Paddle price id or `pro_yearly`. */
  planKey: string;
  amount: number;
  currency: string;
}

export type PlanLimits = Record<"mesh.machines" | "workspaces" | "mem.items" | "teams" | "agent.tabs" | "neighbors", number | null>;

export interface PlanOffer {
  id: string;
  name: string;
  tagline: string;
  features: string[];
  limits?: PlanLimits;
  prices: Partial<Record<BillingInterval, PlanPrice>>;
  recommended?: boolean;
}

export interface PlanCatalog {
  offers: PlanOffer[];
  changesAvailable?: boolean;
}

/** Where offers and prices come from: the built-in table now, the billing provider later. */
export interface PlanCatalogSource {
  load(signal?: AbortSignal): Promise<PlanCatalog>;
}

export interface OfferPrice {
  headline: string;
  per: string;
  note: string;
  planKey: string | null;
}

/** The compiled free baseline by entitlement limit key, copied for display; a test pins it to the signed vectors. */
export const FREE_LIMITS = {
  "mesh.machines": 2,
  workspaces: 2,
  "mem.items": 500,
  teams: 6,
  "agent.tabs": 35,
} as const;

// Display only: entitlements come from the signed server record, never from this table.
export const BUILTIN_PLAN_CATALOG: PlanCatalog = {
  offers: [
    {
      id: "free",
      limits: { ...FREE_LIMITS, neighbors: 0 },
      name: "Free",
      tagline: "Everything you need on one or two machines.",
      features: [],
      prices: {},
    },
    {
      id: "pro",
      limits: { "mesh.machines": 5, workspaces: 7, "mem.items": 1500, teams: 10, "agent.tabs": 70, neighbors: 5 },
      name: "Pro",
      tagline: "More room for your agents and Neighbors.",
      features: [],
      prices: {
        month: { planKey: "pro_monthly", amount: 9.99, currency: "USD" },
        year: { planKey: "pro_yearly", amount: 72, currency: "USD" },
      },
      recommended: true,
    },
    {
      id: "max",
      limits: { "mesh.machines": 12, workspaces: 20, "mem.items": 5000, teams: null, "agent.tabs": null, neighbors: 20 },
      name: "Max",
      tagline: "Space for larger projects and Teams.",
      features: [],
      prices: {
        month: { planKey: "max_monthly", amount: 24.99, currency: "USD" },
        year: { planKey: "max_yearly", amount: 180, currency: "USD" },
      },
    },
    {
      id: "sponsor",
      limits: { "mesh.machines": null, workspaces: null, "mem.items": null, teams: null, "agent.tabs": null, neighbors: null },
      name: "Sponsor",
      tagline: "Support the people building Domios.",
      features: ["Sponsor role in Discord", "Priority support"],
      prices: {
        month: { planKey: "sponsor_monthly", amount: 75, currency: "USD" },
        year: { planKey: "sponsor_yearly", amount: 900, currency: "USD" },
      },
    },
  ],
};

export const builtinCatalogSource: PlanCatalogSource = {
  load: async () => BUILTIN_PLAN_CATALOG,
};

/** A provider-backed source; `fetchJson` returns the raw payload, which must parse or the load fails. */
export function remoteCatalogSource(fetchJson: (signal?: AbortSignal) => Promise<unknown>): PlanCatalogSource {
  return {
    async load(signal) {
      const catalog = parsePlanCatalog(await fetchJson(signal));
      if (!catalog) throw new Error("Plan catalog payload is invalid");
      return catalog;
    },
  };
}

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;
const isText = (value: unknown): value is string => typeof value === "string" && value.length > 0;

function parsePrice(value: unknown): PlanPrice | null {
  if (!isRecord(value) || !isText(value.planKey) || !isText(value.currency)) return null;
  if (typeof value.amount !== "number" || !Number.isFinite(value.amount) || value.amount < 0) return null;
  return { planKey: value.planKey, amount: value.amount, currency: value.currency };
}

export function parsePlanLimits(value: Record<string, unknown>): PlanLimits {
  const limits = {} as PlanLimits;
  for (const key of ["mesh.machines", "workspaces", "mem.items", "teams", "agent.tabs", "neighbors"] as const) {
    const max = value[key];
    if (max !== null && (typeof max !== "number" || !Number.isSafeInteger(max) || max < 0)) throw new Error("Invalid tier limit");
    limits[key] = max;
  }
  return limits;
}

function parseOffer(value: unknown): PlanOffer | null {
  if (!isRecord(value) || !isText(value.id) || !isText(value.name)) return null;
  const prices: PlanOffer["prices"] = {};
  if (isRecord(value.prices)) {
    for (const interval of ["month", "year"] as const) {
      if (value.prices[interval] === undefined) continue;
      const price = parsePrice(value.prices[interval]);
      if (!price) return null;
      prices[interval] = price;
    }
  }
  return {
    id: value.id,
    name: value.name,
    tagline: typeof value.tagline === "string" ? value.tagline : "",
    features: Array.isArray(value.features) ? value.features.filter(isText) : [],
    prices,
    ...(isRecord(value.limits) ? { limits: parsePlanLimits(value.limits) } : {}),
    ...(value.recommended === true ? { recommended: true } : {}),
  };
}

export function parsePlanCatalog(value: unknown): PlanCatalog | null {
  if (!isRecord(value) || !Array.isArray(value.offers)) return null;
  const offers = value.offers.map(parseOffer);
  if (offers.length === 0 || offers.some((offer) => offer === null)) return null;
  return { offers: offers as PlanOffer[] };
}

export function formatMoney(amount: number, currency: string): string {
  const whole = Number.isInteger(amount);
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency,
    minimumFractionDigits: whole ? 0 : 2,
    maximumFractionDigits: whole ? 0 : 2,
  }).format(amount);
}

export function yearlySavingsPercent(offer: PlanOffer): number | null {
  const month = offer.prices.month;
  const year = offer.prices.year;
  if (!month || !year || month.amount <= 0 || month.currency !== year.currency) return null;
  const percent = Math.round((1 - year.amount / (month.amount * 12)) * 100);
  return percent > 0 ? percent : null;
}

export function bestYearlySavings(catalog: PlanCatalog): number | null {
  const all = catalog.offers.map(yearlySavingsPercent).filter((value): value is number => value !== null);
  return all.length ? Math.max(...all) : null;
}

/** The headline is always a monthly figure; a yearly price is shown as its monthly share. */
export function offerPrice(offer: PlanOffer, interval: BillingInterval): OfferPrice {
  const preferred = offer.prices[interval];
  const price = preferred ?? offer.prices.year ?? offer.prices.month;
  if (!price) return { headline: formatMoney(0, "USD"), per: "/ month", note: "Free forever", planKey: null };
  const money = (amount: number) => formatMoney(amount, price.currency);
  if (price !== offer.prices.year) return { headline: money(price.amount), per: "/ month", note: "Billed monthly", planKey: price.planKey };
  const savings = yearlySavingsPercent(offer);
  const billed = `Billed ${money(price.amount)} yearly`;
  const note = !preferred ? `${billed}, yearly only` : savings ? `${billed}, save ${savings}%` : billed;
  return { headline: money(Math.round((price.amount / 12) * 100) / 100), per: "/ month", note, planKey: price.planKey };
}

export type PlanCardAction = { kind: "choose"; planKey: string } | { kind: "manage" } | { kind: "none" };

/** A live subscription never starts a second checkout: any other paid card or interval routes to Manage subscription. */
export function planCardAction(catalog: PlanCatalog, offer: PlanOffer, interval: BillingInterval, subscribedPlan: string | null): PlanCardAction {
  const planKey = offerPrice(offer, interval).planKey;
  if (!planKey) return { kind: "none" };
  if (!subscribedPlan) return { kind: "choose", planKey };
  const sameOffer = offerIdForPlan(catalog, subscribedPlan) === offer.id;
  return sameOffer && (subscribedPlan === planKey || subscribedPlan === offer.id) ? { kind: "none" } : { kind: "manage" };
}

/** The recommended offer's checkout key for the interval on screen, falling back to whichever interval it sells. */
export function upgradePlanKey(catalog: PlanCatalog, interval: BillingInterval): string | null {
  const prices = catalog.offers.find((offer) => offer.recommended)?.prices;
  return (prices?.[interval] ?? prices?.year ?? prices?.month)?.planKey ?? null;
}

/** Matches a subscription plan key such as `pro_yearly` or a bare `pro` to its offer. */
export function offerIdForPlan(catalog: PlanCatalog, plan: string | null | undefined): string {
  if (!plan) return "free";
  return catalog.offers.find((offer) => offer.id === plan || Object.values(offer.prices).some((price) => price?.planKey === plan))?.id ?? plan;
}

export function planChangePrice(offer: PlanOffer, interval: BillingInterval | null): PlanPrice | null {
  const price = interval ? offer.prices[interval] : undefined;
  return price?.priceId ? price : null;
}

/** A benefit line: a limit keeps its amount apart from the noun, and a null amount means unlimited. */
export type PlanFeature = { kind: "limit"; amount: number | null; noun: string } | { kind: "text"; text: string };

// Short enough that every line stays on one line in the narrowest plan card.
const LIMIT_NOUNS: Record<keyof PlanLimits, string> = { "mesh.machines": "machines in your mesh", workspaces: "workspaces",
  "mem.items": "memory items", teams: "Teams", "agent.tabs": TAB_LIMIT_NOUN, neighbors: "Neighbors" };

export function planFeatures(offer: PlanOffer): PlanFeature[] {
  const limits = offer.limits;
  const extras = offer.features.map((text): PlanFeature => ({ kind: "text", text }));
  if (!limits) return extras;
  return [...Object.entries(LIMIT_NOUNS).flatMap(([key, noun]): PlanFeature[] => {
    const amount = limits[key as keyof PlanLimits];
    return key === "neighbors" && amount === 0 ? [] : [{ kind: "limit", amount, noun }];
  }), ...extras];
}
