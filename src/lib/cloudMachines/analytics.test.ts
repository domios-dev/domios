import { describe, expect, it } from "vitest";
import { isProductEvent } from "../productAnalytics";
import {
  cloudHoursPurchasedEvent,
  cloudPaywallShownEvent,
  toCloudAmountBucket,
  toCloudPackId,
} from "./analytics";

describe("cloudMachines analytics helpers", () => {
  it("classifies known pack ids and collapses unknown strings to other", () => {
    expect(toCloudPackId("pack_5")).toBe("pack_5");
    expect(toCloudPackId("pack_10")).toBe("pack_10");
    expect(toCloudPackId("pack_25")).toBe("pack_25");
    expect(toCloudPackId("custom")).toBe("custom");
    expect(toCloudPackId("enterprise_secret_pack")).toBe("other");
    expect(toCloudPackId("")).toBe("other");
    expect(toCloudPackId(undefined)).toBe("other");
  });

  it("buckets purchase amounts without exposing exact cents", () => {
    expect(toCloudAmountBucket(0)).toBe("usd_under_10");
    expect(toCloudAmountBucket(500)).toBe("usd_under_10");
    expect(toCloudAmountBucket(999)).toBe("usd_under_10");
    expect(toCloudAmountBucket(1000)).toBe("usd_10_to_24");
    expect(toCloudAmountBucket(2499)).toBe("usd_10_to_24");
    expect(toCloudAmountBucket(2500)).toBe("usd_25_to_49");
    expect(toCloudAmountBucket(4999)).toBe("usd_25_to_49");
    expect(toCloudAmountBucket(5000)).toBe("usd_50_plus");
    expect(toCloudAmountBucket(50000)).toBe("usd_50_plus");
  });

  it("builds valid catalog events for paywall shown and hours purchased", () => {
    const paywall = cloudPaywallShownEvent("free");
    expect(paywall).toEqual({
      event_type: "cloud_paywall_shown",
      event_properties: { mode: "free" },
    });
    expect(isProductEvent(paywall)).toBe(true);

    const purchased = cloudHoursPurchasedEvent("custom", 1500);
    expect(purchased).toEqual({
      event_type: "cloud_hours_purchased",
      event_properties: { pack_id: "custom", amount_bucket: "usd_10_to_24" },
    });
    expect(isProductEvent(purchased)).toBe(true);
  });
});
