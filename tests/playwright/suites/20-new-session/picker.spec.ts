/**
 * WHAT: picker covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 20-new-session.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "../../helpers/fixtures";
import { isStubTransportActive } from "../../helpers/stub-gate";
import { ApiClient, type Tab } from "../../helpers/api-client";
import { PlanRegistry } from "../../helpers/plan-cleanup";

const TEST_WS_PREFIX = "pw-picker";

const NEW_SESSION_CHOICE_STORAGE_KEY = "codeterm:new-session-choice";

interface MuxWorkspace {
  id: string;
  name: string;
  /** Tab ids in order; the entities live in `MuxState.tabs`. */
  tabs: string[];
  active_tab_id?: string | null;
  /** The protected GENERAL workspace renders as `general-section`, not a card body. */
  protected?: boolean;
}
interface MuxTab {
  id: string;
  role?: string | null;
  parent_tab_id?: string | null;
  agent_type?: string | null;
  title: string;
  hidden?: boolean;
  show_in_tabbar?: boolean;
}
/** `/agents/providers` — the descriptor that owns the model defaults the picker resolves. */
interface PickerProvider {
  id: string;
  default_model?: string | null;
  available_models?: { id: string }[];
}
interface MuxState {
  workspaces: MuxWorkspace[];
  tabs: Record<string, MuxTab>;
  active_workspace_id?: string | null;
  focused_tab_id?: string | null;
}

async function muxState(api: ApiClient): Promise<MuxState> {
  let lastError: unknown;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      return await api.get<MuxState>("/mux/state");
    } catch (e) {
      lastError = e;
    }
  }
  throw lastError;
}

async function createTestWorkspace(api: ApiClient, prefix: string): Promise<{ id: string; name: string }> {
  const name = `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  return { id: ws.id, name };
}

type SidebarLocator = ReturnType<Page["locator"]>;

function workspaceCard(page: Page, wsId: string): SidebarLocator {
  return page.locator(`[data-testid="workspace-card"][data-workspace-id="${wsId}"]`);
}
/** The one expanded workspace body (Sidebar.tsx renders it `hidden` unless selected). */
function workspaceDetail(page: Page, wsId: string): SidebarLocator {
  return page.locator(`[data-testid="workspace-detail"][data-workspace-id="${wsId}"]`);
}

/**
 * Selecting the card is what un-hides a workspace body — the sidebar shows
 * exactly one at a time. Never rely on a freshly created workspace happening to
 * be the selected one: POST /workspaces defaults `activate` to false
 * (dispatch/mod.rs), so selection is otherwise whatever the instance already held.
 */
async function expandSection(page: Page, testId: string): Promise<void> {
  const toggle = page.locator(`[data-testid="${testId}"]`);
  if ((await toggle.count()) === 0) return;
  if ((await toggle.getAttribute("aria-expanded")) === "false") await toggle.click();
}

async function selectWorkspace(page: Page, wsId: string): Promise<void> {
  await expandSection(page, "workspaces-section-toggle");
  const card = workspaceCard(page, wsId);
  await expect(card).toBeVisible({ timeout: 15_000 });
  if ((await card.getAttribute("data-selected")) !== "true") {
    // The select button is the only one on an unselected card; match its own
    // accessible-name prefix rather than recomposing the workspace name here.
    await card.getByRole("button", { name: /^Select workspace / }).click();
  }
  await expect(card).toHaveAttribute("data-selected", "true", { timeout: 10_000 });
  await expect(workspaceDetail(page, wsId)).toBeVisible({ timeout: 10_000 });
}

/** Opens the VIRTUAL picker; no backend tab/PTY exists until confirm. */
async function openPicker(page: Page, wsId: string): Promise<void> {
  await selectWorkspace(page, wsId);
  await workspaceDetail(page, wsId).locator('[data-testid="sidebar-new-session"]').click();
  await expect(page.locator('[data-testid="new-session-picker"]')).toBeVisible({ timeout: 10_000 });
}

/**
 * The picker is a `fixed inset-0 z-[1000]` modal and the desktop `ui` session is
 * WORKER-SCOPED, so one left open covers the viewport for every test after it —
 * including tests in later spec files. Teardown must dismiss it; the `ui` fixture's
 * handover probe reports such a leak by design and never clears it.
 *
 * Every close path (Escape, backdrop, this button) is gated on the picker's `busy`
 * flag, so a teardown landing mid-spawn waits for the product's own enabled signal
 * rather than racing it.
 */
async function dismissPicker(page: Page): Promise<void> {
  const picker = page.locator('[data-testid="new-session-picker"]');
  const close = page.locator('[data-testid="picker-close"]');
  await expect
    .poll(async () => (await picker.count()) === 0 || (await close.isEnabled().catch(() => false)), {
      timeout: 30_000,
    })
    .toBe(true);
  if ((await picker.count()) === 0) return;
  await close.click();
  await expect(
    picker,
    "a picker left open at teardown covers the viewport and blocks every later test on this worker-scoped page",
  ).toBeHidden({ timeout: 10_000 });
}

/** A shared DropdownSelect trigger — replaces the old native <select>. */
function dropdown(page: Page, testId: string) {
  return page.locator(`[data-testid="${testId}"]`);
}
/** Opens a DropdownSelect and clicks the option carrying `value`. */
async function selectDropdown(page: Page, testId: string, value: string): Promise<void> {
  await dropdown(page, testId).click();
  await page.locator(`[data-testid="${testId}-option-${value}"]`).click();
}
interface DropdownOption {
  value: string;
  disabled: boolean;
}

/** Snapshots an OPEN dropdown's options in one evaluation — see `readAttributes`. */
async function readDropdownOptions(page: Page, testId: string): Promise<DropdownOption[]> {
  const prefix = `${testId}-option-`;
  return page.locator(`[data-testid^="${prefix}"]`).evaluateAll(
    (nodes, p) =>
      nodes.map((node) => ({
        value: (node.getAttribute("data-testid") ?? "").slice(p.length),
        disabled: (node as HTMLButtonElement).disabled === true,
      })),
    prefix,
  );
}

async function clickOption(page: Page, testId: string, value: string): Promise<void> {
  await page.locator(`[data-testid="${testId}-option-${value}"]`).click();
}

/**
 * A role's model control is disabled when its provider publishes no models OR that
 * provider's CLI is absent (OrchestrationPairForm.tsx:78) — deliberate, since you
 * cannot pick a model for an agent you cannot launch. Which providers are installed
 * is machine-dependent AND resolves after mount (`probeProviderInstallMap` only ever
 * ADDS disabled ids), so the usable set is not knowable at one instant. Converge on
 * it instead of assuming the default provider happens to be installed here.
 */
async function settleOnUsableModel(page: Page, providerTestId: string, modelTestId: string): Promise<void> {
  const tried = new Set<string>();
  await expect
    .poll(
      async () => {
        if (await dropdown(page, modelTestId).isEnabled()) return true;
        // Retiring each provider we land on guarantees termination; retrying the
        // same one would spin until the deadline and blame the wrong thing.
        tried.add((await dropdown(page, providerTestId).getAttribute("data-value")) ?? "");
        await dropdown(page, providerTestId).click();
        const next = (await readDropdownOptions(page, providerTestId)).find(
          (o) => o.value && !o.disabled && !tried.has(o.value),
        );
        if (!next) {
          await page.keyboard.press("Escape");
          throw new Error(
            `${providerTestId}: no installed provider publishes a model to choose (tried ${[...tried].join(", ")})`,
          );
        }
        await clickOption(page, providerTestId, next.value);
        return dropdown(page, modelTestId).isEnabled();
      },
      { timeout: 30_000 },
    )
    .toBe(true);
}

/** Opens a DropdownSelect and clicks the first option whose value differs from
 *  the current one; returns the value it landed on. */
async function selectFirstDifferentOption(page: Page, testId: string): Promise<string> {
  const current = await dropdown(page, testId).getAttribute("data-value");
  await dropdown(page, testId).click();
  const next = (await readDropdownOptions(page, testId)).find(
    (o) => o.value && o.value !== current && !o.disabled,
  );
  if (!next) {
    await page.keyboard.press("Escape");
    // Returning `current` here would let the caller "prove" persistence by
    // round-tripping an unchanged value — a pass that asserts nothing.
    throw new Error(`${testId} offers no selectable alternative to "${current}"`);
  }
  await clickOption(page, testId, next.value);
  return next.value;
}

function tabCount(tabs: Tab[]): number {
  return tabs.length;
}
function ptyCount(tabs: Tab[]): number {
  return tabs.filter((p) => p.pty_id != null).length;
}

function visibleTabsInWorkspace(state: MuxState, wsId: string): MuxTab[] {
  const ws = state.workspaces.find((w) => w.id === wsId);
  return (ws?.tabs ?? [])
    .map((tabId) => state.tabs[tabId])
    .filter((tab): tab is MuxTab => Boolean(tab) && tab.hidden !== true && tab.show_in_tabbar !== false);
}

/** Reads PEER_ORCH_GREETING straight from the Rust source and splits it on the
 *  `{planner}` placeholder, so greeting assertions track Puccini's wording
 *  revision at the source instead of pinning a literal (round-18 addendum). */
function repoFile(rel: string): string {
  let dir = process.cwd();
  for (let i = 0; i < 8; i++) {
    const candidate = path.join(dir, rel);
    if (existsSync(candidate)) return candidate;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error(`could not locate ${rel} from ${process.cwd()}`);
}
function peerOrchGreetingSegments(): string[] {
  const src = readFileSync(repoFile("src-tauri/src/prompts.rs"), "utf8");
  const m = src.match(/PEER_ORCH_GREETING\s*:\s*&str\s*=\s*"([\s\S]*?)"\s*;/);
  if (!m) throw new Error("PEER_ORCH_GREETING not found in prompts.rs");
  const raw = m[1].replace(/\\\s+/g, ""); // undo Rust string line-continuations
  return raw
    .split("{planner}")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

async function waitForOrchestrator(api: ApiClient, wsId: string): Promise<MuxTab> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const s = await muxState(api);
    const ws = s.workspaces.find((w) => w.id === wsId);
    const tabId = ws?.tabs.find((id) => s.tabs[id]?.role === "Orchestrator");
    if (tabId && s.tabs[tabId]?.agent_type) return s.tabs[tabId];
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`no orchestrator agent materialized in workspace ${wsId}`);
}

async function waitForSingleAgent(api: ApiClient, wsId: string): Promise<MuxTab> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const s = await muxState(api);
    const ws = s.workspaces.find((w) => w.id === wsId);
    const tabId = ws?.tabs.find((id) => s.tabs[id]?.agent_type);
    if (tabId) return s.tabs[tabId];
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`no single agent materialized in workspace ${wsId}`);
}

function tabRows(page: Page, wsId: string): SidebarLocator {
  return workspaceDetail(page, wsId).locator('[data-testid="tab-item"]');
}
/** `data-tab-id` also lands on the terminal pane, so anchor on the sidebar row. */
function sidebarTabRow(page: Page, wsId: string, tabId: string): SidebarLocator {
  return workspaceDetail(page, wsId).locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`);
}

/**
 * One-shot snapshot of an attribute across every match. `count()` then `nth(i)`
 * is NOT atomic: the sidebar re-renders as backend state propagates, so a node
 * counted a moment ago can detach before it is read, and `getAttribute` then
 * waits for a re-attachment that never comes — the whole matched set is read in
 * a single evaluation instead.
 */
async function readAttributes(locator: SidebarLocator, attribute: string): Promise<string[]> {
  return locator.evaluateAll((nodes, attr) => nodes.map((node) => node.getAttribute(attr) ?? ""), attribute);
}

/** Titles from each row's `title` attribute (first line; tooltip is `title\ndescription`). */
async function readTabTitles(rows: SidebarLocator): Promise<string[]> {
  return (await readAttributes(rows, "title")).map((t) => t.split("\n")[0]);
}

/** Asserts the workspace body exists before reading it — an absent body used to
 *  return [] and turn every title assertion into a silent no-op. */
async function sidebarTabTitles(page: Page, wsId: string): Promise<string[]> {
  await expect(
    workspaceDetail(page, wsId),
    `workspace ${wsId} renders no sidebar body, so its tab titles cannot be asserted`,
  ).toHaveCount(1);
  return readTabTitles(tabRows(page, wsId));
}

/** A team-anchoring tab (Orchestrator / General Agent) has no row of its own —
 *  its Team card head stands for it, so the ghost sweep must read both. */
function teamNames(scope: SidebarLocator): SidebarLocator {
  return scope.locator('[data-testid="team-name"]');
}

interface SidebarScope {
  label: string;
  wsId: string;
  rows: SidebarLocator;
  teams: SidebarLocator;
}

/** Every sidebar surface that renders tabs: the protected GENERAL section (which
 *  carries no `data-workspace-id`) plus one body per ordinary workspace. */
async function sidebarTabScopes(page: Page, state: MuxState, bodyIds: string[]): Promise<SidebarScope[]> {
  const scopes: SidebarScope[] = [];
  const generalWs = state.workspaces.find((w) => w.protected === true);
  if (generalWs) {
    const general = page.locator('[data-testid="general-section"]');
    await expect(general, "the protected GENERAL workspace renders no sidebar section").toHaveCount(1);
    scopes.push({
      label: "general-section",
      wsId: generalWs.id,
      rows: general.locator('[data-testid="tab-item"]'),
      teams: teamNames(general),
    });
  }
  for (const id of bodyIds) {
    scopes.push({
      label: `workspace ${id}`,
      wsId: id,
      rows: workspaceDetail(page, id).locator('[data-testid="tab-item"]'),
      teams: teamNames(workspaceDetail(page, id)),
    });
  }
  return scopes;
}
function isPlaceholderTitle(title: string): boolean {
  const t = title.trim();
  return t.length === 0 || t === "…" || t === "...";
}

async function cleanupWorkspace(api: ApiClient, wsId: string | undefined): Promise<void> {
  if (!wsId) return;
  await api.closeWorkspace(wsId).catch(() => {});
}

test.describe("new-session picker", () => {
  // Spawn-flow tests run on the stub transport; picker UI plumbing is
  // provider-independent, so golden carries no live-LLM cost here.
  async function requireStub(api: import("../../helpers/api-client").ApiClient) {
    test.skip(!(await isStubTransportActive(api)), "dev daemon is not running with CODETERM_STUB_AGENT_CMD");
  }

  // scenarios that spawn real agents raise their own timeout explicitly.
  test.describe.configure({ timeout: 30_000 });

  let ws: { id: string; name: string };
  // Id-diff registry catches bare "Orchestration"/timestamp title regressions.
  // Closing a workspace does not cascade to plans, so spawn scenarios need this.
  const plans = new PlanRegistry();

  test.beforeAll(async () => {
    await plans.snapshot();
  });

  test.beforeEach(async ({ api, ui }) => {
    ws = await createTestWorkspace(api, TEST_WS_PREFIX);
    await openPicker(ui.page, ws.id);
  });

  test.afterEach(async ({ api, ui }) => {
    // Dismiss BEFORE touching state under the modal, and before the workspace it
    // targets disappears. Runs even when the test body threw — which is the leak
    await dismissPicker(ui.page);
    // Sweep created plans BEFORE closing the workspace (close doesn't cascade).
    await plans.sweep();
    await cleanupWorkspace(api, ws?.id);
  });

  test.afterAll(async () => {
    await plans.sweep();
    const leaked = await plans.created();
    expect(
      leaked,
      `suite must leave ZERO created plans (before == after). Leaked: ${leaked
        .map((p) => `${p.plan_id}:${JSON.stringify(p.title)}`)
        .join(", ")}`,
    ).toHaveLength(0);
    const junk = await plans.junk();
    expect(
      junk,
      `no orchmatrix-*/bare 'Orchestration'/timestamp junk may remain. Found: ${junk
        .map((p) => `${p.plan_id}:${JSON.stringify(p.title)}`)
        .join(", ")}`,
    ).toHaveLength(0);
  });

  test("@golden OPEN: New session shows the VIRTUAL picker, no backend tab/PTY", async ({ api, ui }) => {
    const tabs = await api.tabs();
    const state = await muxState(api);
    expect(visibleTabsInWorkspace(state, ws.id), "picker must not create a rendered backend tab").toHaveLength(0);
    // …and no snapshot tab carries the virtual picker's title.
    expect(
      Object.values(state.tabs).some((p) => p.title === "New session"),
      "no picker tab may appear in the snapshot",
    ).toBe(false);
    // The virtual picker renders NO sidebar row at all — not even a titled one.
    expect(
      await sidebarTabTitles(ui.page, ws.id),
      "the virtual picker must not render a sidebar tab row",
    ).toEqual([]);
    // Sanity: the picker tab is virtual, so /tabs never lists it.
    expect(tabs.some((p) => p.title === "New session")).toBe(false);
  });

  test("@golden MODE CARDS: exactly one card selected at a time, pair-affordance follows it", async ({ ui }) => {
    const page = ui.page;
    const single = page.locator('[data-testid="picker-mode-single"]');
    const pair = page.locator('[data-testid="picker-mode-orchestration"]');
    await expect(single).toBeVisible();
    await expect(pair).toBeVisible();

    await pair.click();
    await expect(pair).toHaveAttribute("data-selected", "true");
    await expect(single).toHaveAttribute("data-selected", "false");
    await expect(page.locator('[data-testid="pair-affordance"]')).toBeVisible();

    await single.click();
    await expect(single).toHaveAttribute("data-selected", "true");
    await expect(pair).toHaveAttribute("data-selected", "false");
  });

  /**
   * NOT COVERED: "orchestrator=Opus, planner=Fable". `defaultNewSessionChoice`
   * (src/lib/newSessionChoice.ts) gives BOTH heads `providers[0]` and that one
   * provider's `default_model`; no role-specific default exists anywhere in src/,
   * and "fable" appears nowhere in the frontend. Only src/help/content/en.ts still
   * promises the split. That contradiction is a product question, not a spec one,
   * so this asserts the resolution mechanism that does hold and leaves the split
   * claim unasserted rather than freezing either side of the disagreement.
   */
  test("@golden DEFAULTS: with no prior persistence both heads open on one provider at its declared default model", async ({ api, ui }) => {
    const page = ui.page;
    await page.evaluate((key) => window.localStorage.removeItem(key), NEW_SESSION_CHOICE_STORAGE_KEY);
    await page.reload();
    await openPicker(page, ws.id);
    // The old spec CLICKED the pair card here, which masked whether pair is the
    // unpersisted default at all. defaultNewSessionChoice returns mode "pair".
    await expect(page.locator('[data-testid="picker-mode-orchestration"]')).toHaveAttribute("data-selected", "true");

    const orchProvider = await dropdown(page, "picker-provider-orchestrator").getAttribute("data-value");
    const plannerProvider = await dropdown(page, "picker-provider-planner").getAttribute("data-value");
    expect(orchProvider, "an unpersisted picker must preselect a provider").toBeTruthy();
    expect(plannerProvider, "both heads share the unpersisted default provider").toBe(orchProvider);

    const provider = (await api.get<PickerProvider[]>("/agents/providers")).find((p) => p.id === orchProvider);
    expect(provider, `picker offers provider "${orchProvider}" which /agents/providers does not list`).toBeTruthy();
    const offered = (provider?.available_models ?? []).map((m) => m.id);
    const declaredDefault = provider?.default_model ?? "";
    expect(
      offered,
      `provider "${orchProvider}" must declare a default_model it actually offers, else nothing pins the picker's preselection`,
    ).toContain(declaredDefault);

    await expect(dropdown(page, "picker-model-orchestrator")).toHaveAttribute("data-value", declaredDefault);
    await expect(dropdown(page, "picker-model-planner")).toHaveAttribute("data-value", declaredDefault);
  });

  test("@golden WORKSPACE ROW: preselected to current, help tooltip is instant", async ({ ui }) => {
    const page = ui.page;
    await expect(dropdown(page, "picker-workspace")).toHaveAttribute("data-value", ws.id);

    const help = page.locator('[data-testid="workspace-help"]');
    await help.hover();
    const tooltip = page.locator('[data-testid="workspace-help-tooltip"]');
    await expect(tooltip).toBeVisible({ timeout: 100 });
    await expect(tooltip).toContainText(/workspace/i);
  });

  test("@golden PREVIEW LINE: reflects mode + destination, updates live on workspace change", async ({ api, ui }) => {
    const page = ui.page;
    const otherWs = await createTestWorkspace(api, "pw-picker-preview-dest");
    try {
      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      const preview = page.locator('[data-testid="picker-preview"]');
      await expect(preview).toContainText("Orchestrator + Planner");
      await expect(preview).toContainText(ws.name);

      await selectDropdown(page, "picker-workspace", otherWs.id);
      await expect(preview).toContainText(otherWs.name);
      await expect(preview).not.toContainText(ws.name);
    } finally {
      await cleanupWorkspace(api, otherWs.id);
    }
  });

  test("@golden PERSISTENCE: mode + model persist, workspace resets to current", async ({ api, ui }) => {
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    // Changing a model needs a role whose model control is actually usable here.
    await settleOnUsableModel(page, "picker-provider-planner", "picker-model-planner");
    const plannerProvider = (await dropdown(page, "picker-provider-planner").getAttribute("data-value")) ?? "";
    const changedModel = await selectFirstDifferentOption(page, "picker-model-planner");
    const otherWs = await createTestWorkspace(api, "pw-picker-persist-dest");
    try {
      await selectDropdown(page, "picker-workspace", otherWs.id);

      // localStorage choice; reopen a fresh picker in the ORIGINAL workspace.
      await page.reload();
      await openPicker(page, ws.id);
      await expect(page.locator('[data-testid="picker-mode-orchestration"]')).toHaveAttribute("data-selected", "true");
      await expect(dropdown(page, "picker-provider-planner")).toHaveAttribute("data-value", plannerProvider);
      await expect(dropdown(page, "picker-model-planner")).toHaveAttribute("data-value", changedModel);
      await expect(dropdown(page, "picker-workspace")).toHaveAttribute("data-value", ws.id);
    } finally {
      await cleanupWorkspace(api, otherWs.id);
    }
  });

  test("@golden IN-PLACE PAIR SPAWN: confirm materializes orchestrator + planner, no husk left behind", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    // Fresh workspace: no backend tabs before confirm (picker is virtual).
    const before = await muxState(api);
    expect(visibleTabsInWorkspace(before, ws.id)).toHaveLength(0);

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    // The picker retires; the group (orchestrator + planner child) appears.
    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(2);

    const orchestrator = await waitForOrchestrator(api, ws.id);
    expect(orchestrator.role).toBe("Orchestrator");
    // No leftover virtual husk anywhere in the snapshot.
    const after = await muxState(api);
    expect(Object.values(after.tabs).some((p) => p.title === "New session")).toBe(false);

    // Plan creation is the live orchestrator's first act (lazy, LLM-driven) —
    // covered in picker-live.spec.ts; the stub transport verifies pair plumbing.
  });

  test("@golden GREETING: orchestrator's first assistant message is the PEER_ORCH_GREETING, unprompted", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    const tab = await waitForOrchestrator(api, ws.id);
    const segments = peerOrchGreetingSegments();
    expect(segments.length, "PEER_ORCH_GREETING yielded no constant segment to assert").toBeGreaterThan(0);

    // `subscriptionId` is REQUIRED and camelCase (ChatSubscriptionBody,
    // api/web_routes.rs:659-663). A bodyless call is rejected by the extractor
    const subscriptionId = `pw-picker-greeting-${tab.id}`;
    const chatStart = () =>
      api.post<{ messages?: { type?: string; content?: string }[] }>(
        `/tabs/${tab.id}/chat-raw/start`,
        { subscriptionId },
      );
    // Unguarded and up front: a contract error must surface HERE carrying its own
    // status text. Swallowing it into "" spent the poll's whole deadline and then
    expect((await chatStart()).messages, "chat-raw/start must answer with a message window").toBeDefined();

    await expect
      .poll(async () => (await chatStart()).messages?.[0]?.content ?? "", { timeout: 60_000 })
      // the subordinate-framing reword tracks automatically.
      .toContain(segments[0]);
    const greeting = (await chatStart()).messages?.[0]?.content ?? "";
    for (const seg of segments) {
      expect(greeting, `greeting must contain constant segment "${seg}"`).toContain(seg);
    }
  });

  test("@golden CROSS-WORKSPACE: spawning into workspace B leaves NO husk in A", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    const wsB = await createTestWorkspace(api, "pw-picker-cross-b");
    try {
      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      await selectDropdown(page, "picker-workspace", wsB.id);
      await page.locator('[data-testid="picker-confirm"]').click();

      await expect
        .poll(async () => visibleTabsInWorkspace(await muxState(api), wsB.id).length, {
          timeout: 60_000,
        })
        .toBeGreaterThanOrEqual(2);
      const orchestrator = await waitForOrchestrator(api, wsB.id);
      expect(orchestrator.role, "orchestrator must land in workspace B").toBe("Orchestrator");

      // …and A keeps NO husk: the virtual picker just vanished, no backend tab.
      const after = await muxState(api);
      expect(visibleTabsInWorkspace(after, ws.id)).toHaveLength(0);
    } finally {
      await cleanupWorkspace(api, wsB.id);
    }
  });

  test("@golden SINGLE SELECT: Start closes the picker and selects + focuses the created agent tab", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-single"]').click();
    // No post-confirm permissions dialog exists any more: full access is the
    // picker's own "Full access for agents" checkbox (NewSessionPicker.tsx).
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
    const tab = await waitForSingleAgent(api, ws.id);
    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.active_tab_id ?? null, {
        timeout: 30_000,
      })
      .toBe(tab.id);
    const s = await muxState(api);
    expect(s.active_workspace_id, "active workspace is the picker host").toBe(ws.id);
    expect(s.focused_tab_id, "the created agent tab is focused").toBe(tab.id);
    // No lingering virtual picker tab in the workspace.
    expect(s.workspaces.find((w) => w.id === ws.id)?.tabs.every((id) => id !== "virtual"), "no virtual tab").toBe(true);
  });

  test("@golden PAIR SELECT: Start selects + focuses the ORCHESTRATOR tab of the new group", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
    const orchestrator = await waitForOrchestrator(api, ws.id);
    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.active_tab_id ?? null, {
        timeout: 30_000,
      })
      .toBe(orchestrator.id);
    const s = await muxState(api);
    expect(s.active_workspace_id, "active workspace is the picker host").toBe(ws.id);
    // Stub agents boot in seconds, so the planner's focus grab can land after
    // the product refocuses the orchestrator — assert the settled state.
    await expect
      .poll(async () => (await muxState(api)).focused_tab_id, { timeout: 30_000 })
      .toBe(orchestrator.id);

    await ui.page.screenshot({ path: "tests/playwright/artifacts/picker-pair-selected.png" });
  });

  test("@golden CROSS-WORKSPACE SELECT: Start switches the active workspace to B and selects its orchestrator", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    const wsB = await createTestWorkspace(api, "pw-picker-cross-sel-b");
    try {
      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      await selectDropdown(page, "picker-workspace", wsB.id);
      await page.locator('[data-testid="picker-confirm"]').click();

      await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
      const orchestrator = await waitForOrchestrator(api, wsB.id);
      await expect
        .poll(async () => (await muxState(api)).active_workspace_id ?? null, { timeout: 30_000 })
        .toBe(wsB.id);
      const s = await muxState(api);
      expect(s.workspaces.find((w) => w.id === wsB.id)?.active_tab_id, "B's orchestrator tab is active").toBe(orchestrator.id);
      await expect
      .poll(async () => (await muxState(api)).focused_tab_id, { timeout: 30_000 })
      .toBe(orchestrator.id);
      // Host A keeps no husk.
      expect(visibleTabsInWorkspace(s, ws.id)).toHaveLength(0);
    } finally {
      await cleanupWorkspace(api, wsB.id);
    }
  });

  /**
   * A pair mints its own Team (service/orchestration_pair.rs), so the sidebar
   * renders a TeamUnit: the orchestrator ANCHORS the team and therefore has no
   * row of its own (SidebarWorkspaceTabs buckets exclude `anchorsTeam` tabs) —
   * the card head stands for it, and its close runs through the Team menu.
   */
  test("@golden GROUP CLOSE: a manager row closes on its own; closing the team head confirms before anything closes", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(2);

    const orchTab = await waitForOrchestrator(api, ws.id);
    const state = await muxState(api);
    const plannerTab = visibleTabsInWorkspace(state, ws.id).find((t) => t.id !== orchTab.id)!;

    const teamUnit = workspaceDetail(page, ws.id).locator('[data-testid="team-unit"]');
    await expect(teamUnit, "the spawned pair renders as exactly one Team").toHaveCount(1);
    expect(
      await sidebarTabRow(page, ws.id, orchTab.id).count(),
      "the orchestrator anchors the Team, so it must not also render a member row",
    ).toBe(0);

    const plannerRow = sidebarTabRow(page, ws.id, plannerTab.id);
    await expect(plannerRow, "the planner is the Team's manager row").toBeVisible({ timeout: 15_000 });
    await plannerRow.hover();
    await plannerRow.locator('button[title="Tab actions"]').click();
    const plannerClose = page.locator('[data-testid="tab-menu-close"]');
    await expect(plannerClose).toBeVisible({ timeout: 5_000 });
    await plannerClose.click();

    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(1);

    await teamUnit.locator('[data-testid="team-menu-trigger"]').click();
    const closeTeam = page.locator('[data-testid="team-menu"]').getByRole("menuitem", { name: "Close Team" });
    await expect(closeTeam).toBeVisible({ timeout: 5_000 });
    await closeTeam.click();

    const confirm = page.locator('[data-testid="group-close-confirm"]');
    await expect(confirm, "closing a Team head must confirm, never close silently").toBeVisible({ timeout: 5_000 });
    expect(
      visibleTabsInWorkspace(await muxState(api), ws.id),
      "the head must still be open while the confirmation is pending",
    ).toHaveLength(1);
    await confirm.locator('[data-testid="group-close-confirm-button"]').click();

    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(0);
  });

  test("@golden SHARED DROPDOWN + VIRTUAL PHYSICALITY (scenario 11)", async ({ api, ui }) => {
    const page = ui.page;
    const picker = page.locator('[data-testid="new-session-picker"]');

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await expect(picker.locator("select")).toHaveCount(0);
    const provider = dropdown(page, "picker-provider-orchestrator");
    await expect(provider).toBeVisible();
    await provider.click();
    // queryable DOM nodes, which a native <select> popup never is.
    await expect(page.locator('[data-testid="picker-provider-orchestrator-option-claude"]')).toBeVisible({ timeout: 5_000 });
    await page.keyboard.press("Escape");
    await expect(picker, "picker stayed mounted — no reload/flicker").toBeVisible();

    await page.reload();
    await expect(picker).toBeHidden();
    const baseTabs = await api.tabs();
    const baseState = await muxState(api);
    const baseVisible = visibleTabsInWorkspace(baseState, ws.id).length;

    await openPicker(page, ws.id);

    const openTabs = await api.tabs();
    expect(tabCount(openTabs), "tab count unchanged while picker open").toBe(tabCount(baseTabs));
    expect(ptyCount(openTabs), "PTY count unchanged while picker open").toBe(ptyCount(baseTabs));
    const openState = await muxState(api);
    expect(
      visibleTabsInWorkspace(openState, ws.id).length,
      "no rendered backend tab added on open",
    ).toBe(baseVisible);
    expect(
      Object.values(openState.tabs).some((p) => p.title === "New session"),
      "snapshot has no picker tab entry",
    ).toBe(false);

    // Ghost-title guard at picker-open: the sidebar renders exactly the backend's
    // visible tabs (no ghost row added) and every one of them is titled.
    const openTitles = await sidebarTabTitles(page, ws.id);
    expect(openTitles, "sidebar rows must match the backend's visible tabs").toHaveLength(baseVisible);
    for (const title of openTitles) {
      expect(isPlaceholderTitle(title), `open-state tab title "${title}" must be titled`).toBe(false);
    }
  });

  test("@golden NO GHOST TITLE across a pair spawn (scenario 12)", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();

    // any empty/"…" placeholder frame the moment it appears. Prove the surface
    // exists once up front — a sampler over an absent scope observes nothing and
    await expect(workspaceDetail(page, ws.id), "no sidebar body to sample for ghosts").toHaveCount(1);
    const rows = tabRows(page, ws.id);
    let ghost: string | null = null;
    let polling = true;
    const poller = (async () => {
      while (polling) {
        for (const title of await readTabTitles(rows)) {
          if (isPlaceholderTitle(title)) ghost = title;
        }
        await new Promise((r) => setTimeout(r, 60));
      }
    })();

    await page.locator('[data-testid="picker-confirm"]').click();
    await waitForOrchestrator(api, ws.id);
    // Let the group settle (planner child materializes + titles resolve).
    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(2);
    polling = false;
    await poller;

    // Settled snapshot: the pair is one Team — the orchestrator as the card head,
    // the planner as its single member row. Both surfaces must carry a real name.
    const settled = await sidebarTabTitles(page, ws.id);
    expect(settled, "the planner is the Team's one member row").toHaveLength(1);
    const cardNames = (await teamNames(workspaceDetail(page, ws.id)).allTextContents()).map((t) => t.trim());
    expect(cardNames, "the orchestrator renders as exactly one Team card head").toHaveLength(1);
    for (const title of [...settled, ...cardNames]) {
      expect(isPlaceholderTitle(title), `settled sidebar entry "${title}" must be titled`).toBe(false);
    }
    expect(ghost, `a placeholder tab title "${ghost}" appeared mid-spawn`).toBeNull();
  });
});

/**
 * Ghost-tab invariant (task_63c585953fef4dbdb9e1d906077f8b18)
 *
 * These tests verify that no sidebar tab ever renders as a bare "…"/empty
 * placeholder — whether from snapshot restore, a leaked virtual tab, or a
 * blank backend tab that lost its title. The Rust unit tests in
 * snapshot.rs prove the root fix (skip_snapshot); these Playwright tests
 * guard the invariant at the UI layer on a live dev session.
 */
test.describe("ghost-tab invariant", () => {
  test.describe.configure({ timeout: 30_000, retries: 0 });

  // The skip_snapshot scenario opens a picker and asserts against it; nothing in
  // its flow closes it, and a mid-test failure would leak it into the next file.
  test.afterEach(async ({ ui }) => {
    await dismissPicker(ui.page);
  });

  test("@golden GHOST-TAB: current session sidebar has no placeholder titles", async ({ api, ui }) => {
    // tab restored with a blank title, this catches it immediately.
    // A collapsed disclosure renders no rows; expand rather than sweep an empty DOM.
    await expandSection(ui.page, "workspaces-section-toggle");
    await expandSection(ui.page, "general-section-toggle");

    // Coverage first, or the sweep below silently inspects nothing. The sidebar
    // trails backend state by an event, so assert the SETTLED correspondence:
    const bodies = ui.page.locator('[data-testid="workspace-detail"]');
    let bodyIds: string[] = [];
    let ordinaryIds: string[] = [];
    await expect
      .poll(
        async () => {
          ordinaryIds = (await muxState(api)).workspaces
            .filter((candidate) => candidate.protected !== true)
            .map((candidate) => candidate.id)
            .sort();
          bodyIds = (await readAttributes(bodies, "data-workspace-id")).filter(Boolean).sort();
          return bodyIds.join(",") === ordinaryIds.join(",");
        },
        {
          timeout: 15_000,
          message:
            "the sidebar must settle on exactly one body per ordinary workspace — a missing one hides tabs from this sweep, a stale one is a leak",
        },
      )
      .toBe(true);
    expect(bodyIds, "settled sidebar bodies must match the backend's ordinary workspaces").toEqual(ordinaryIds);

    const state = await muxState(api);
    const scopes = await sidebarTabScopes(ui.page, state, bodyIds);
    expect(scopes.length, "the sidebar must expose at least one tab surface").toBeGreaterThan(0);

    for (const scope of scopes) {
      const entries = [
        ...(await readTabTitles(scope.rows)),
        ...(await scope.teams.allTextContents()).map((t) => t.trim()),
      ];
      // GENERAL is protected and always holds its General Agent, rendered either
      // as a row or as the Team card that stands in for an anchoring tab. An empty
      if (scope.label === "general-section") {
        expect(entries.length, "GENERAL renders no sidebar entry to inspect").toBeGreaterThan(0);
      }
      for (const title of entries) {
        expect(
          isPlaceholderTitle(title),
          `${scope.label} sidebar entry "${title}" is a ghost placeholder`,
        ).toBe(false);
      }
    }
  });

  test("@golden GHOST-TAB: skip_snapshot tab is absent from backend snapshot after picker creates it", async ({ api, ui }) => {
    const testWs = await createTestWorkspace(api, `${TEST_WS_PREFIX}-ghost-snap`);
    try {
      await openPicker(ui.page, testWs.id);

      // Virtual picker open: backend must have ZERO rendered tabs in this workspace.
      const liveState = await muxState(api);
      expect(
        visibleTabsInWorkspace(liveState, testWs.id),
        "virtual picker must not create a rendered backend tab (would restore as ghost on restart)",
      ).toHaveLength(0);

      // …and the sidebar body agrees: no row at all, ghost or otherwise.
      expect(
        await sidebarTabTitles(ui.page, testWs.id),
        "an open virtual picker must leave the workspace body empty",
      ).toEqual([]);
    } finally {
      await api.closeWorkspace(testWs.id).catch(() => {});
    }
  });
});
