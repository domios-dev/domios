// WHAT: Dialog readiness, outcome and explicit pair selections.
// WHY: A reconciled picker model must not silently authorize a paid pair.
// ORACLE: The confirm spy stays untouched when either rendered role differs.
import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { closePickerScript, focusedPaneScript, openPickerScript, startTeamScript } from "./cdp.mjs";

function fakeDialog({ confirmEnabled = true, onConfirm, choice = null, renderedChoice = choice }) {
  const observers = new Set();
  const notify = () => queueMicrotask(() => observers.forEach((callback) => callback()));
  const alerts = [];
  const team = { pressed: "false", getAttribute: () => team.pressed, click() { team.pressed = "true"; notify(); } };
  const confirm = { disabled: !confirmEnabled, click() { onConfirm({ picker, alerts }); notify(); } };
  const controls = Object.fromEntries([["orchestratorProviderId", "picker-provider-orchestrator"], ["plannerProviderId", "picker-provider-planner"],
    ["orchestratorModelId", "picker-model-orchestrator"], ["plannerModelId", "picker-model-planner"]]
    .map(([key, testId]) => ['[data-testid="' + testId + '"]', { getAttribute: () => renderedChoice?.[key] }]));
  const picker = {
    isConnected: true,
    querySelector: (selector) => ({ '[data-testid="picker-mode-orchestration"]': team, '[data-testid="picker-confirm"]': confirm })[selector] ?? controls[selector] ?? null,
    querySelectorAll: (selector) => (selector === '[role="alert"]' ? alerts : []),
  };
  const context = vm.createContext({
    setTimeout, clearTimeout, queueMicrotask,
    window: { localStorage: { getItem: () => JSON.stringify(choice) } },
    document: { documentElement: {}, querySelector: () => (picker.isConnected ? picker : null) },
    MutationObserver: class { constructor(callback) { this.callback = callback; } observe() { observers.add(this.callback); } disconnect() { observers.delete(this.callback); } },
  });
  return { context, team };
}

test("startTeam selects Team mode, confirms, and reports a closed dialog as success", async () => {
  const { context, team } = fakeDialog({ onConfirm: ({ picker }) => { picker.isConnected = false; } });
  const outcome = await vm.runInContext(startTeamScript(1_000), context);
  assert.equal(team.pressed, "true");
  assert.deepEqual({ ...outcome }, { ok: true });
});

test("startTeam reports the dialog's refusal alert", async () => {
  const { context } = fakeDialog({ onConfirm: ({ alerts }) => alerts.push({ textContent: "Manager and Worker Tabs need an explicit declaration" }) });
  const outcome = await vm.runInContext(startTeamScript(1_000), context);
  assert.equal(outcome.ok, false);
  assert.match(outcome.alert, /explicit declaration/);
});

test("startTeam fails on its deadline when Start never enables", async () => {
  const { context } = fakeDialog({ confirmEnabled: false, onConfirm: () => {} });
  await assert.rejects(vm.runInContext(startTeamScript(20), context), /deadline: Team mode selected and Start enabled/);
});

test("page scripts compile and JSON-escape caller values", () => {
  const scripts = [
    focusedPaneScript('t"1', 10),
    openPickerScript({ workspaceId: 'w"1', workspaceCwd: "/tmp/a b", choice: { mode: "pair", orchestratorProviderId: "codex", plannerProviderId: "codex" } }, 10),
    closePickerScript(null), closePickerScript('{"mode":"single"}'), startTeamScript(10),
  ];
  for (const script of scripts) assert.doesNotThrow(() => new vm.Script(script));
  assert.match(scripts[0], /t\\"1/);
});

const strictChoice = { mode: "pair", orchestratorProviderId: "arbitrary", plannerProviderId: "arbitrary",
  orchestratorModelId: "exact/model", plannerModelId: "exact/model" };

test("strict UI pair confirms only when both rendered roles preserve exact selections", async () => {
  let calls = 0;
  const { context } = fakeDialog({ choice: strictChoice, onConfirm: ({ picker }) => { calls++; picker.isConnected = false; } });
  assert.equal((await vm.runInContext(startTeamScript(1000, strictChoice), context)).ok, true);
  assert.equal(calls, 1);
});

test("strict UI pair rejects either role substitution before confirm", async () => {
  for (const key of ["orchestratorProviderId", "plannerProviderId", "orchestratorModelId", "plannerModelId"]) {
    let calls = 0;
    const { context } = fakeDialog({ choice: strictChoice, renderedChoice: { ...strictChoice, [key]: "substitution" },
      onConfirm: () => { calls++; } });
    await assert.rejects(vm.runInContext(startTeamScript(1000, strictChoice), context), /model policy refused/);
    assert.equal(calls, 0);
  }
});
