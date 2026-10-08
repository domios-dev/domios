import { strict as assert } from "node:assert";
import { test } from "node:test";
import { supporterActivity } from "./SupporterSpendPanel";

test("supporter activity shows compact counts with exact values in the tooltip", () => {
  const activity = supporterActivity(1530, 48210);
  assert.match(activity.text, /1\.5k req\/hr/);
  assert.match(activity.text, /48k req\/day/);
  assert.match(activity.title, /1,530/);
  assert.match(activity.title, /48,210/);
});
