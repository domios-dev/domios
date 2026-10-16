import assert from "node:assert/strict";
import { test } from "vitest";
import { cronRowMenu } from "./cronRowMenu";

test("cron actions use explicit protection and enabled state", () => {
  for (const enabled of [true, false]) {
    for (const protectedJob of [true, false]) {
      const [toggle, remove] = cronRowMenu({ enabled, protected: protectedJob });
      assert.equal(toggle.action, "toggle");
      assert.equal(toggle.label, enabled ? "Disable" : "Enable");
      assert.equal(toggle.disabled, false);
      assert.equal(remove.disabled, protectedJob);
      assert.equal(remove.label, protectedJob ? "System job cannot be deleted" : "Delete cron");
    }
  }
  assert.equal(cronRowMenu({ enabled: true })[1].disabled, false);
});
