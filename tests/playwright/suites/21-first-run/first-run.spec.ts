/**
 * WHAT: first run covers the core arrival flow — the General-agent picker surface and a live General Agent.
 * WHY: Every user walks this path exactly once and the harness previously DISMISSED it instead of asserting it.
 * ORACLE: Backend state from /mux/state and /agents/providers, plus the product's own render gate; never a spinner.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "../../helpers/fixtures";
import { repoRoot } from "../../helpers/dev-instance";
import { isStubTransportActive } from "../../helpers/stub-gate";
import type { ApiClient } from "../../helpers/api-client";

const TEST_WS_PREFIX = "pw-first-run";
const CARD_TESTID_PREFIX = "agent-picker-card-";

interface MuxWorkspace {
  id: string;
  name: string;
  /** Tab ids in order; the entities live in `MuxState.tabs`. */
  tabs: string[];
  protected?: boolean;
}
interface MuxTab {
  id: string;
  title: string;
  description?: string | null;
  protected?: boolean;
  hidden?: boolean;
  pty_id?: string | null;
  agent_type?: string | null;
  /** Serialized only once bound (`skip_serializing_if = "Option::is_none"`). */
  agent_session_id?: string | null;
}
interface MuxState {
  workspaces: MuxWorkspace[];
  tabs: Record<string, MuxTab>;
  /** Idle | Working | Waiting | Dead, per PTY-backed tab. */
  tab_statuses?: Record<string, string>;
}
/** `/agents/providers` — `transport` is the discriminant the picker filters on. */
interface PickerProvider {
  id: string;
  transport?: string;
}

function muxState(api: ApiClient): Promise<MuxState> {
  return api.get<MuxState>("/mux/state");
}

/**
 * Constants are read from the sources that own them rather than retyped here:
 * the daemon writes `FIRST_RUN_TAB_MARKER` onto the tab it mints at boot, and the
 * frontend's render gate is `description === FIRST_RUN_GA_TAB_MARKER`. A spec that
 * hardcoded the literal would keep passing after either side renamed it.
 */
function constantFromSource(relative: string, pattern: RegExp): string {
  const file = path.join(repoRoot(), relative);
  const match = readFileSync(file, "utf8").match(pattern);
  if (!match) throw new Error(`could not read a required constant from ${relative}`);
  return match[1];
}

function backendFirstRunMarker(): string {
  return constantFromSource(
    "src-tauri/src/service/general_agent/first_run.rs",
    /FIRST_RUN_TAB_MARKER\s*:\s*&str\s*=\s*"([^"]+)"/,
  );
}

function frontendFirstRunMarker(): string {
  return constantFromSource(
    "src/components/AgentPicker/constants.ts",
    /FIRST_RUN_GA_TAB_MARKER\s*=\s*"([^"]+)"/,
  );
}

function generalTabTitle(): string {
  return constantFromSource(
    "src-tauri/src/service/general_agent/mod.rs",
    /GENERAL_TAB_TITLE\s*:\s*&str\s*=\s*"([^"]+)"/,
  );
}

async function createTestWorkspace(api: ApiClient, prefix: string): Promise<{ id: string; name: string }> {
  const name = `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  return { id: ws.id, name };
}

/** A tab wearing the first-run marker; the daemon mints an equivalent one at boot. */
async function createFirstRunMarkerTab(api: ApiClient, wsId: string, marker: string): Promise<string> {
  const created = await api.post<{ id: string }>("/tabs", {
    workspace_id: wsId,
    title: "Get started",
    description: marker,
  });
  expect(created.id, "POST /tabs must answer with the created tab id").toBeTruthy();
  // The render gate reads `description` off the snapshot, so prove the backend kept it.
  await expect
    .poll(async () => (await muxState(api)).tabs[created.id]?.description ?? null, {
      timeout: 15_000,
      message: "the created tab must carry the first-run marker the daemon's own boot path writes",
    })
    .toBe(marker);
  return created.id;
}

/**
 * One-shot snapshot of an attribute across every match: `count()` then `nth(i)` is
 * not atomic while the picker re-renders as the availability probe resolves.
 */
function readAttributes(locator: ReturnType<Page["locator"]>, attribute: string): Promise<string[]> {
  return locator.evaluateAll((nodes, attr) => nodes.map((node) => node.getAttribute(attr) ?? ""), attribute);
}

function eligibleProviderIds(providers: PickerProvider[]): string[] {
  return providers.filter((p) => p.transport === "pty").map((p) => p.id).sort();
}

/** The one protected workspace the General Agent boot path owns. */
function generalWorkspace(state: MuxState): MuxWorkspace {
  const owned = state.workspaces.filter((w) => w.protected === true);
  expect(
    owned.map((w) => w.name),
    "the boot path must leave exactly one protected General workspace",
  ).toHaveLength(1);
  return owned[0];
}

/** The protected General tab inside it — the tab the General Agent binds to. */
function generalTab(state: MuxState): MuxTab {
  const ws = generalWorkspace(state);
  const candidates = ws.tabs
    .map((tabId) => state.tabs[tabId])
    .filter((tab): tab is MuxTab => Boolean(tab) && tab.protected === true && tab.hidden !== true);
  expect(
    candidates.map((tab) => tab.title),
    `the General workspace ${ws.id} must hold exactly one protected General tab`,
  ).toHaveLength(1);
  return candidates[0];
}

/**
 * A surviving marker tab suppresses New session for every later test in this
 * worker (Sidebar.tsx gates `handleNewSession` on `firstRunActive`), so the sweep
 * is asserted rather than hoped for.
 */
async function closeMarkerTab(api: ApiClient, tabId: string | undefined): Promise<void> {
  if (!tabId) return;
  await api.closeTab(tabId).catch(() => {});
  await expect
    .poll(async () => Boolean((await muxState(api)).tabs[tabId]), {
      timeout: 15_000,
      message: "a leaked first-run marker tab disables New session across the whole worker",
    })
    .toBe(false);
}

test.describe("first run", () => {
  test.describe.configure({ timeout: 90_000 });

  let createdWorkspaceId: string | undefined;
  let createdMarkerTabId: string | undefined;

  test.afterEach(async ({ api }) => {
    await closeMarkerTab(api, createdMarkerTabId);
    createdMarkerTabId = undefined;
    if (createdWorkspaceId) await api.closeWorkspace(createdWorkspaceId).catch(() => {});
    createdWorkspaceId = undefined;
  });

  test("@golden FIRST RUN SURFACE: the marker tab renders the General-agent picker over exactly the PTY providers", async ({
    api,
    ui,
  }) => {
    const marker = backendFirstRunMarker();
    expect(
      frontendFirstRunMarker(),
      "the daemon writes the marker and the frontend gates on it — a rename on either side must not pass silently",
    ).toBe(marker);

    const workspace = await createTestWorkspace(api, TEST_WS_PREFIX);
    createdWorkspaceId = workspace.id;
    const tabId = await createFirstRunMarkerTab(api, workspace.id, marker);
    createdMarkerTabId = tabId;
    await api.focusWorkspace(workspace.id);
    await api.focusTab(tabId);

    const page = ui.page;
    const picker = page.locator(`[data-tab-id="${tabId}"] [data-testid="first-run-general-picker"]`);
    await expect(picker, "a first-run marked tab must render the General-agent picker").toBeVisible({
      timeout: 30_000,
    });
    await expect(picker.locator('[data-testid="agent-provider-picker"]')).toBeVisible();

    // No agent CLI on the machine swaps the card branch for the install fallback.
    // That is a different surface with different assertions, so name it rather than
    await expect(
      picker.locator('[data-testid="agent-picker-install-fallback"]'),
      "this machine reports no installed agent CLI, so the first-run picker's card branch cannot be exercised here",
    ).toHaveCount(0);

    const eligible = eligibleProviderIds(await api.get<PickerProvider[]>("/agents/providers"));
    expect(
      eligible,
      "/agents/providers must publish at least one pty provider, else the picker has nothing to offer",
    ).not.toHaveLength(0);

    const cards = picker.locator(`[data-testid^="${CARD_TESTID_PREFIX}"]`);
    await expect(cards, "one card per eligible provider").toHaveCount(eligible.length);
    const rendered = (await readAttributes(cards, "data-testid"))
      .map((id) => id.slice(CARD_TESTID_PREFIX.length))
      .sort();
    expect(rendered, "the picker offers exactly the pty providers the daemon publishes").toEqual(eligible);

    // Preselection resolves only after the install probe answers, so let the
    // web-first matcher converge instead of sampling one instant.
    const selected = picker.locator(`[data-testid^="${CARD_TESTID_PREFIX}"][data-selected="yes"]`);
    await expect(selected, "the picker preselects exactly one provider").toHaveCount(1);
    await expect(
      selected,
      "the preselected provider must be installed — you cannot start an agent you cannot launch",
    ).toHaveAttribute("data-installed", "yes");
    await expect(
      picker.locator('[data-testid="agent-picker-confirm"]'),
      "a resolved picker with an installed selection must be confirmable",
    ).toBeEnabled();

    // NOT COVERED here, deliberately: clicking confirm calls
    // complete_general_agent_first_run, which rewrites general_agent.agent_provider,
  });

  test("@golden FIRST RUN GATES NEW SESSION: backend first-run state disables New session until the tab is retired", async ({
    api,
    ui,
  }) => {
    const page = ui.page;
    const workspace = await createTestWorkspace(api, `${TEST_WS_PREFIX}-gate`);
    createdWorkspaceId = workspace.id;
    const tabId = await createFirstRunMarkerTab(api, workspace.id, backendFirstRunMarker());
    createdMarkerTabId = tabId;
    await api.focusWorkspace(workspace.id);

    const toggle = page.locator('[data-testid="workspaces-section-toggle"]');
    if ((await toggle.count()) > 0 && (await toggle.getAttribute("aria-expanded")) === "false") {
      await toggle.click();
    }
    const card = page.locator(`[data-testid="workspace-card"][data-workspace-id="${workspace.id}"]`);
    await expect(card).toBeVisible({ timeout: 15_000 });
    if ((await card.getAttribute("data-selected")) !== "true") {
      await card.getByRole("button", { name: /^Select workspace / }).click();
    }
    await expect(card).toHaveAttribute("data-selected", "true", { timeout: 10_000 });
    const detail = page.locator(`[data-testid="workspace-detail"][data-workspace-id="${workspace.id}"]`);
    await expect(detail).toBeVisible({ timeout: 10_000 });

    const sessionPicker = page.locator('[data-testid="new-session-picker"]');
    const newSession = detail.locator('[data-testid="sidebar-new-session"]');
    const markerRow = detail.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`);
    await expect(markerRow, "the marker tab must render a sidebar row before the gate is probed").toBeVisible({
      timeout: 15_000,
    });

    // The product does not silently swallow the click — it DISABLES the control, so
    // the refusal is one a user and a screen reader can both perceive. Its wording is
    await expect(
      newSession,
      "New session must be refused while a first-run tab is outstanding",
    ).toBeDisabled({ timeout: 15_000 });
    await expect(newSession).toHaveAttribute("aria-disabled", "true");
    await expect(sessionPicker, "no picker may be mounted while first run is pending").toHaveCount(0);

    // Retiring the tab is an observable backend transition, not elapsed time.
    await closeMarkerTab(api, tabId);
    createdMarkerTabId = undefined;
    await expect(markerRow).toHaveCount(0, { timeout: 15_000 });

    // Positive control: the disable was caused by the first-run tab and nothing
    // ambient. Without this a permanently dead button would "prove" the gate.
    await expect(
      newSession,
      "with no first-run tab left, New session must become available again",
    ).toBeEnabled({ timeout: 15_000 });
    await newSession.click();
    await expect(
      sessionPicker,
      "the re-enabled control must actually open the picker — otherwise the gate above proves nothing",
    ).toBeVisible({ timeout: 15_000 });
    const close = page.locator('[data-testid="picker-close"]');
    await expect(close).toBeEnabled({ timeout: 30_000 });
    await close.click();
    await expect(
      sessionPicker,
      "a picker left open covers the viewport for every later test on this worker-scoped page",
    ).toBeHidden({ timeout: 10_000 });
  });

  test("@golden GENERAL AGENT LIVE: the protected General tab owns a bound agent session, not a spinner", async ({
    api,
  }) => {
    const state = await muxState(api);
    const general = generalTab(state);
    expect(general.title, "the General tab keeps the title its boot path assigns").toBe(generalTabTitle());

    const agentType = general.agent_type ?? null;
    expect(agentType, "a started General Agent reports the provider it launched").not.toBeNull();
    expect(
      (await api.providers()).map((p) => p.id),
      `the General tab reports agent_type "${agentType}", which /agents/providers does not publish`,
    ).toContain(agentType);
    expect(general.pty_id, "a started General Agent owns a PTY").toBeTruthy();
    expect(state.tab_statuses?.[general.id], "a started General Agent is not a dead tab").not.toBe("Dead");

    // The binding is the real oracle: a session id bound to THIS tab means the
    // agent reported a session, not that a loading state went away.
    await expect
      .poll(async () => (await muxState(api)).tabs[general.id]?.agent_session_id ?? null, {
        timeout: 60_000,
        message: "the General Agent must bind an agent session to its own tab",
      })
      .not.toBeNull();
  });

  test("@golden GENERAL AGENT RESTART: a restart replaces the PTY and rebinds a live session on the same tab", async ({
    api,
  }) => {
    // The General Agent runs the configured provider for real. Gated at runtime so
    // golden keeps the test, and so a restart never burns provider credit: with
    test.skip(!(await isStubTransportActive(api)), "dev daemon is not running with CODETERM_STUB_AGENT_CMD");
    test.setTimeout(180_000);

    const before = generalTab(await muxState(api));
    expect(before.pty_id, "the General Agent must be live before a restart can be observed").toBeTruthy();

    await api.post("/agents/general/restart");

    // restart_general_agent tears the PTY down and clears the binding before
    // relaunching, so a new PTY id is the product's own evidence it really restarted.
    await expect
      .poll(async () => (await muxState(api)).tabs[before.id]?.pty_id ?? null, {
        timeout: 120_000,
        message: "a restarted General Agent must run on a new PTY",
      })
      .not.toBe(before.pty_id);
    await expect
      .poll(async () => (await muxState(api)).tabs[before.id]?.agent_session_id ?? null, {
        timeout: 120_000,
        message: "a restarted General Agent must rebind an agent session",
      })
      .not.toBeNull();

    // Asserting the restore, not just the restart: the shared instance is handed on
    // with the same protected General tab, on the same provider, alive.
    const after = await muxState(api);
    const restored = generalTab(after);
    expect(restored.id, "a restart must reuse the General tab, never mint a new one").toBe(before.id);
    expect(restored.protected, "the General tab stays protected across a restart").toBe(true);
    expect(restored.agent_type, "a restart must not change the configured provider").toBe(before.agent_type);
    expect(after.tab_statuses?.[restored.id], "the restarted General Agent is not dead").not.toBe("Dead");
  });
});
