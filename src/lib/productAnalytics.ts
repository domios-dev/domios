import { PRODUCT_EVENT_CATALOG, PRODUCT_EVENT_ENUMS, type ProductEvent, type Feature } from "./productAnalytics.generated";

export type AnalyticsEndpoint = (method: "POST", path: "/api/analytics/event", event: ProductEvent) => Promise<unknown>;

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function catalogValue(type: string, value: unknown): boolean {
  if (type === "Version") return typeof value === "string" && /^\d{1,5}\.\d{1,5}\.\d{1,5}$/.test(value) && value.split(".").every(part => Number(part) <= 65535);
  if (type === "u32") return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 4294967295;
  if (type.startsWith("Option<")) return value === null || catalogValue(type.slice(7, -1), value);
  const values: Readonly<Record<string, readonly string[]>> = PRODUCT_EVENT_ENUMS;
  return typeof value === "string" && Object.prototype.hasOwnProperty.call(values, type) && values[type].includes(value);
}

export function isProductEvent(value: unknown): value is ProductEvent {
  if (!object(value) || Object.keys(value).length !== 2 || typeof value.event_type !== "string" || !object(value.event_properties)) return false;
  const catalog: Readonly<Record<string, Readonly<Record<string, string>>>> = PRODUCT_EVENT_CATALOG;
  if (!Object.prototype.hasOwnProperty.call(catalog, value.event_type)) return false;
  const fields = catalog[value.event_type];
  const properties = value.event_properties;
  return Object.keys(properties).length === Object.keys(fields).length && Object.entries(fields).every(([field, type]) => Object.prototype.hasOwnProperty.call(properties, field) && catalogValue(type, properties[field]));
}

export async function emitProductEvent(endpoint: AnalyticsEndpoint, consent: "off" | "on", event: ProductEvent): Promise<void> {
  if (consent !== "on" || !isProductEvent(event)) return;
  try { await endpoint("POST", "/api/analytics/event", event); }
  catch { /* Analytics cannot interrupt a product action. */ }
}

export type FeatureEndpoint = <T>(method: string, path: string, body?: unknown) => Promise<T>;

export async function emitFeatureUsed(endpoint: FeatureEndpoint, feature: Feature): Promise<void> {
  try {
    const status = await endpoint<{ consent: "off" | "on" }>("GET", "/api/analytics/status");
    await emitProductEvent(endpoint, status.consent, { event_type: "feature_used", event_properties: { feature } });
  } catch { /* Analytics cannot interrupt a product action. */ }
}
