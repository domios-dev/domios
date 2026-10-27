import { expect, it } from "vitest";
import { giftFailure, parseGiftReceipt } from "./giftRedemptionModel";

const finite = { schema_version: 1, status: "redeemed", plan: "max", duration: "1m", expires_at: 1_900_000_000 };

it.each(["1m", "3m", "1y", "3y", "lifetime"])("accepts the server's %s duration without computing an expiry", duration => {
  const receipt = { ...finite, duration, expires_at: duration === "lifetime" ? null : 1_900_000_000 };
  expect(parseGiftReceipt(receipt)).toEqual(receipt);
});

it.each(Object.keys(finite))("refuses a response missing %s", field => {
  const incomplete: Record<string, unknown> = { ...finite };
  delete incomplete[field];
  expect(() => parseGiftReceipt(incomplete)).toThrow("invalid_response");
});

it.each([
  { ...finite, schema_version: 2 }, { ...finite, status: "pending" }, { ...finite, plan: "admin" },
  { ...finite, duration: "12m" }, { ...finite, expires_at: null }, { ...finite, expires_at: -1 },
  { ...finite, duration: "lifetime" }, { ...finite, expires_at: 8_640_000_000_001 },
  { ...finite, grant: "unverified" }, null,
  { ...finite, plan: ["pro"] }, { ...finite, duration: ["1m"] },
])("refuses malformed or expanded receipts", value => {
  expect(() => parseGiftReceipt(value)).toThrow("invalid_response");
});

it("unknown backend reasons cannot become user-visible key material", () => {
  expect(giftFailure("opaque-private-material")).not.toContain("opaque-private-material");
  expect(giftFailure("already_subscribed")).toBe(giftFailure("unavailable"));
});
