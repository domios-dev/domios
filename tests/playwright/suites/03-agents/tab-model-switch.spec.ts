/**
 * WHAT: the sidebar row's kebab switches a running agent's model in session.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 03-agents.
 * ORACLE: the daemon's own tab output buffer receives the provider's in-agent switch
 * command carrying the exact model id the provider descriptor published, the id was
 * provably absent before the click, and the tab keeps the session it had — proving the
 * switch is in-session and not a restart. Not proven here: that the vendor CLI actually
 * changed model, which is provider behaviour and unreachable under a substitute.
 */
import { expect, test } from "../../helpers/fixtures";
import type { ApiClient, Provider } from "../../helpers/api-client";
import { pollUntil } from "../../helpers/poll";
import { tempSpawnCwd } from "../../helpers/spawn-cwd";
import { createStubWorkspace, isStubTransportActive, stubTabOutput } from "../../helpers/stub-gate";

/** Providers that publish both halves of a live switch: the in-agent command
 *  template and a catalog with something to switch to. Capability, not identity. */
function switchableProviders(providers: Provider[]): Provider[] {
  return providers.filter(
    (provider) =>
      Boolean(provider.model_switch_format?.trim()) && (provider.available_models?.length ?? 0) >= 2,
  );
}

async function tabById(api: ApiClient, tabId: string) {
  const tab = (await api.tabs()).find((candidate) => candidate.id === tabId);
  if (!tab) throw new Error(`tab ${tabId} is gone from /tabs`);
  return tab;
}

test("@golden @stub the row kebab switches the agent model in session", async ({
  api,
  ui,
  spawnAgent,
}) => {
  test.setTimeout(120_000);
  test.skip(!(await isStubTransportActive(api)), "dev daemon is not running with CODETERM_STUB_AGENT_CMD");

  const candidates = switchableProviders(await api.providers());
  expect(
    candidates.length,
    "no provider publishes model_switch_format plus a two-entry catalog, so the row menu has nothing to offer",
  ).toBeGreaterThan(0);

  const workspace = await createStubWorkspace(api);
  const page = ui.page;
  try {
    // First candidate that actually boots under the substitute owns the run; a
    // provider whose recorded cast is missing is a finding, not a reason to skip.
    let booted: { provider: Provider; tabId: string } | null = null;
    const bootFailures: string[] = [];
    for (const provider of candidates) {
      const spawnModel = provider.available_models![0];
      const spawned = await spawnAgent({
        agent: provider.id,
        model: spawnModel.id,
        title: `pw-model-${provider.id}-${Date.now()}`,
        workspace_id: workspace.id,
        skip_permissions: true,
        activate: true,
        cwd: tempSpawnCwd("pw-model-switch-"),
      });
      const up = await pollUntil(
        async () => (await stubTabOutput(api, spawned.tab_id)).includes("stub replay"),
        30_000,
        250,
        `${provider.id} stub banner`,
      ).catch(() => false);
      if (up) {
        booted = { provider, tabId: spawned.tab_id };
        break;
      }
      bootFailures.push(provider.id);
      await api.closeTab(spawned.tab_id).catch(() => {});
    }
    expect(
      booted,
      `no switchable provider reached the substitute; tried ${bootFailures.join(", ") || "none"}`,
    ).not.toBeNull();
    const { provider, tabId } = booted!;

    // The asserted value is read off the descriptor that owns it — never recomposed
    // here — and must be provably absent from the buffer before the click, so a
    const before = await stubTabOutput(api, tabId);
    const spawnModelId = provider.available_models![0].id;
    const target = provider.available_models!.find(
      (model) => model.id !== spawnModelId && !before.includes(model.id),
    );
    expect(
      target,
      `every published model id of ${provider.id} is already in the tab buffer, so no id can witness the switch`,
    ).toBeTruthy();
    const sessionBefore = (await tabById(api, tabId)).agent_session_id ?? null;

    const row = page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first();
    await expect(row, "the spawned agent has no sidebar row to open a menu on").toBeVisible({
      timeout: 30_000,
    });
    await row.hover();
    await row.locator('button[title="Tab actions"]').click();

    const modelEntry = page.locator('[data-testid="tab-menu-model"]');
    await expect(modelEntry, "the row menu offers no model entry").toBeVisible({ timeout: 10_000 });
    await expect(
      modelEntry,
      "the model entry does not announce that it opens a picker",
    ).toHaveAttribute("aria-haspopup", "menu");
    await modelEntry.click();

    const option = page.locator(`[data-testid="tab-menu-model-${target!.id}"]`);
    await expect(option, `the picker does not offer ${target!.id}`).toBeVisible({ timeout: 10_000 });
    await expect(
      option,
      "the offered model is not exposed as one choice in a set",
    ).toHaveAttribute("role", "menuitemradio");
    await expect(option, "the target model is already marked as the current one").toHaveAttribute(
      "aria-checked",
      "false",
    );
    await option.click();

    await pollUntil(
      async () => (await stubTabOutput(api, tabId)).includes(target!.id),
      30_000,
      250,
      `daemon buffer receives the switch to ${target!.id}`,
    );

    expect(
      (await tabById(api, tabId)).agent_session_id ?? null,
      "the model switch replaced the session instead of switching inside it",
    ).toBe(sessionBefore);
  } finally {
    await api.closeWorkspace(workspace.id).catch(() => {});
  }
});
