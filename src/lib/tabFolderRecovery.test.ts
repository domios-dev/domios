import assert from "node:assert/strict";
import { test } from "node:test";
import { recoverTabFolder } from "./tabFolderRecovery";

test("folder recovery persists the chosen tab folder before resuming its existing session", async () => {
  const calls: unknown[] = [];
  let saved!: () => void;
  const saving = new Promise<void>(resolve => { saved = resolve; });
  const recovery = recoverTabFolder("own-tab", "/chosen", {
    update: async (...args) => { calls.push(args); await saving; },
    restart: async (...args) => { calls.push(args); },
  });
  assert.deepEqual(calls, [["own-tab", "/chosen"]]);
  saved();
  await recovery;
  assert.deepEqual(calls, [["own-tab", "/chosen"], ["own-tab", false]]);
});

test("a refused folder change never restarts the tab", async () => {
  let restarted = false;
  await assert.rejects(recoverTabFolder("own-tab", "/chosen", {
    update: async () => { throw Error("refused"); },
    restart: async () => { restarted = true; },
  }));
  assert.equal(restarted, false);
});
