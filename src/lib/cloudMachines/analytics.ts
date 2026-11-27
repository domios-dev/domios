import type {
  CloudAmountBucket,
  CloudPackId,
  CloudPaywallMode,
  ProductEvent,
} from "../productAnalytics.generated";

const KNOWN_PACK_IDS: ReadonlySet<string> = new Set([
  "pack_5",
  "pack_10",
  "pack_25",
  "custom",
]);

export function toCloudPackId(packId: string | null | undefined): CloudPackId {
  if (packId && KNOWN_PACK_IDS.has(packId)) {
    return packId as CloudPackId;
  }
  return "other";
}

export function toCloudAmountBucket(amountCents: number): CloudAmountBucket {
  if (!Number.isFinite(amountCents) || amountCents < 1000) return "usd_under_10";
  if (amountCents < 2500) return "usd_10_to_24";
  if (amountCents < 5000) return "usd_25_to_49";
  return "usd_50_plus";
}

export function cloudPaywallShownEvent(mode: CloudPaywallMode): ProductEvent {
  return {
    event_type: "cloud_paywall_shown",
    event_properties: { mode },
  };
}

export function cloudHoursPurchasedEvent(packId: string | null | undefined, amountCents: number): ProductEvent {
  return {
    event_type: "cloud_hours_purchased",
    event_properties: {
      pack_id: toCloudPackId(packId),
      amount_bucket: toCloudAmountBucket(amountCents),
    },
  };
}
