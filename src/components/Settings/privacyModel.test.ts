import assert from "node:assert/strict";
import { test } from "node:test";
import { analyticsEnabled, changeAnalyticsConsent, type AnalyticsStatus, type PrivacyApi } from "./privacyModel";

const off: AnalyticsStatus = { consent: "off", queued: 0, has_device_id: false, has_user_id: false };

test("analytics stays off until recorded consent exists", () => {
  assert.equal(analyticsEnabled(null), false);
  assert.equal(analyticsEnabled(off), false);
  assert.equal(analyticsEnabled({ ...off, consent: "on" }), true);
});

test("privacy toggle awaits persisted consent and opt-out clears identity", async () => {
  let stored = off;
  const writes: string[] = [];
  const api: PrivacyApi = {
    async status() { return stored; },
    async consent(consent) {
      writes.push(consent);
      stored = consent === "on" ? { ...off, consent, has_device_id: true } : off;
      return stored;
    },
  };
  assert.equal(analyticsEnabled(await changeAnalyticsConsent(api, true)), true);
  assert.deepEqual(await changeAnalyticsConsent(api, false), off);
  assert.deepEqual(await api.status(), off);
  assert.deepEqual(writes, ["on", "off"]);
});

test("a failed persistence write never yields an optimistic consent state", async () => {
  const api: PrivacyApi = { async status() { return off; }, async consent() { throw new Error("write failed"); } };
  await assert.rejects(changeAnalyticsConsent(api, true));
  assert.equal(analyticsEnabled(await api.status()), false);
});
