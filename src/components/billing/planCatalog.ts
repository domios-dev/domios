export type BillingInterval = "month" | "year";

export interface PlanPrice {
  /** The provider's checkout key for this price, e.g. a Paddle price id or `pro_yearly`. */
  planKey: string;
  amount: number;
  currency: string;
}

export interface PlanOffer {
  id: string;
  name: string;
  tagline: string;
  features: string[];
  prices: Partial<Record<BillingInterval, PlanPrice>>;
  recommended?: boolean;
}

export interface PlanCatalog {
  offers: PlanOffer[];
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

// Display only: entitlements come from the signed server record, never from this table.
export const BUILTIN_PLAN_CATALOG: PlanCatalog = {
  offers: [
    {
      id: "free",
      name: "Free",
      tagline: "Everything you need on one or two machines.",
      features: ["2 machines in your mesh", "2 workspaces", "Agents, memory, teams and plans"],
      prices: {},
    },
    {
      id: "pro",
      name: "Pro",
      tagline: "No limits on machines or workspaces.",
      features: ["Unlimited machines in your mesh", "Unlimited workspaces", "Everything in Free"],
      prices: {
        month: { planKey: "pro_monthly", amount: 5, currency: "USD" },
        year: { planKey: "pro_yearly", amount: 33.6, currency: "USD" },
      },
      recommended: true,
    },
    {
      id: "sponsor",
      name: "Sponsor",
      tagline: "Pro, plus a direct line to the people building Domios.",
      features: ["Everything in Pro", "Sponsor role in Discord", "Priority support"],
      prices: {
        month: { planKey: "sponsor_monthly", amount: 30, currency: "USD" },
        year: { planKey: "sponsor_yearly", amount: 200, currency: "USD" },
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

/** Matches a subscription plan key such as `pro_yearly` or a bare `pro` to its offer. */
export function offerIdForPlan(catalog: PlanCatalog, plan: string | null | undefined): string {
  if (!plan) return "free";
  return catalog.offers.find((offer) => offer.id === plan || Object.values(offer.prices).some((price) => price?.planKey === plan))?.id ?? plan;
}
