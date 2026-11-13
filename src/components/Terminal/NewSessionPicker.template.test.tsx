import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  modelDecisionFixtures, templateListFixture, templatePreviewFixture, templateRecallFixture, templateStartResultFixtures,
} from "../../lib/transport/teamTemplates.testdata";

const fixture = vi.hoisted(() => {
  const workspace = { id: "workspace", name: "Workspace", tabs: [] as string[], active_tab_id: null, cwd: "/workspace", cwd_missing: false };
  return {
    workspace,
    state: { workspaces: [workspace], active_workspace_id: workspace.id, focused_tab_id: null, tabs: {} },
    actions: {
      spawnAgentTab: vi.fn(async () => null),
      createOrchestrationPair: vi.fn(async (_input: unknown) => ({ orchestrator_tab_id: "lead-tab" })),
      selectTabByTab: vi.fn(async () => undefined),
      upsertToast: vi.fn(),
    },
    providers: {
      providers: [{ id: "claude", display_name: "Claude Code", available_models: [], reasoning_efforts: [] }] as never[],
      loading: false,
      refetch: vi.fn(),
    },
    templates: {
      list: vi.fn(),
      recall: vi.fn(),
      preview: vi.fn(),
      export: vi.fn(),
      import: vi.fn(),
      delete: vi.fn(),
      card: vi.fn(),
      distill: vi.fn(),
      file: vi.fn(),
      peerStart: vi.fn(),
    },
    peers: vi.fn(async () => [{ id: "inst_8c1f", name: "eight" }]),
    vault: vi.fn(async () => ["SEARCH_API_KEY"]),
  };
});

vi.mock("../../store/multiplexer/hooks", () => ({ useMuxState: () => fixture.state, useMultiplexer: () => fixture.actions }));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => fixture.providers }));
vi.mock("../../hooks/useModelPresets", () => ({ useModelPresets: () => ({ presets: [], loading: false, error: null }) }));
vi.mock("../../lib/providerInstallStatus", () => ({ probeProviderInstallMap: vi.fn(async () => ({})) }));
vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => undefined),
}));
vi.mock("../../lib/transport/orchestrationRuntime", () => ({ teamTemplatesTransport: () => fixture.templates }));
vi.mock("../../lib/teamTemplates/vaultSecrets", () => ({ listVaultSecretNames: fixture.vault, openSecretsSettings: vi.fn() }));
vi.mock("../../lib/teamTemplates/hostPeers", () => ({ listHostPeers: fixture.peers }));

import { NewSessionPicker } from "./NewSessionPicker";
import { openTemplateFileReview, openTemplatePreview } from "../../lib/teamTemplates/pendingPreview";
import exchange from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/exchange.json";

type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const startablePreview = { ...templatePreviewFixture, where_needed: null };
let root: Root | null = null;
let host: HTMLDivElement;
let onClose = vi.fn();

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "pair", orchestratorProviderId: "claude", plannerProviderId: "claude" }));
  for (const mock of Object.values(fixture.templates)) mock.mockReset();
  fixture.templates.list.mockResolvedValue(templateListFixture);
  fixture.templates.recall.mockResolvedValue(templateRecallFixture);
  fixture.templates.preview.mockResolvedValue(startablePreview);
  fixture.actions.createOrchestrationPair.mockReset();
  fixture.actions.createOrchestrationPair.mockResolvedValue({ orchestrator_tab_id: "lead-tab" });
  onClose = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]')!;
const buttons = () => Array.from(dialog().querySelectorAll<HTMLButtonElement>("button"));
const button = (text: string) => buttons().find((candidate) => candidate.textContent?.trim() === text || candidate.getAttribute("aria-label") === text);
const start = () => dialog().querySelector<HTMLButtonElement>('[data-testid="picker-confirm"]')!;

async function click(target: HTMLElement | undefined) {
  expect(target).toBeDefined();
  await act(async () => target!.click());
}

async function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, value);
    input.dispatchEvent(new window.Event("input", { bubbles: true }));
  });
}

async function choose(labelPrefix: string, option: string) {
  const trigger = dialog().querySelector<HTMLElement>(`[aria-label^="${labelPrefix}"]`);
  expect(trigger).not.toBeNull();
  await act(async () => { trigger!.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true })); });
  await click(Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemradio"]')).find((item) => item.textContent?.trim() === option));
}

async function renderPicker() {
  await act(async () => {
    root!.render(<NewSessionPicker workspaceId="workspace" workspaceCwd="/workspace" onClose={onClose} />);
  });
}

async function chooseResearchDesk() {
  await click(button("Change template"));
  const card = buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"));
  await click(card);
}

describe("Team template start in the session dialog", () => {
  it("starts the default pair without a template, as before", async () => {
    await renderPicker();
    expect(dialog().textContent).toContain("Default team");
    await click(start());
    expect(fixture.actions.createOrchestrationPair).toHaveBeenCalledOnce();
    expect(fixture.actions.createOrchestrationPair.mock.calls[0][0]).not.toHaveProperty("template");
  });

  it("puts the best recalled template first and keeps the default when recall is unavailable", async () => {
    await renderPicker();
    await click(button("Change template"));
    const titles = () => Array.from(dialog().querySelectorAll('[aria-label="Templates"] button[aria-pressed]')).map((candidate) => candidate.textContent ?? "");
    expect(titles()[0]).toContain("Release train");
    expect(titles()[1]).toContain("Default team");
    await act(async () => root!.unmount());
    fixture.templates.recall.mockRejectedValue(new Error("template_unavailable: template recall is not available in this Domios build yet"));
    root = createRoot(host);
    await renderPicker();
    await click(button("Change template"));
    expect(titles()[0]).toContain("Default team");
    expect(fixture.templates.preview).toHaveBeenCalledWith({ name: "dev.domios/default", version: null }, expect.anything());
  });

  it.each([null, "Mira"])("reviews an unverified package from %s before use, then starts it with the decision id, opt-outs and inputs", async (receivedFrom) => {
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, form: { ...startablePreview.form,
      template: { ...startablePreview.form.template, provenance: "imported", verified: false, received_from: receivedFrom } } });
    await renderPicker();
    await chooseResearchDesk();
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).not.toBeNull();
    expect(start()).toBeNull();
    expect(document.activeElement).toBe(dialog().querySelector('[data-testid="template-review-consent"] h2'));
    const workers = dialog().querySelector('section[aria-label="Workers on demand"]')!.textContent;
    expect(workers).toContain("sonnet · Medium");
    expect(workers).toContain("You pick");
    const skill =dialog().querySelector<HTMLButtonElement>('[role="switch"][aria-label="Bring source-check"]')!;
    expect(skill.getAttribute("aria-checked")).toBe("true");
    expect(dialog().querySelector('[data-payload-item="skill:source-check"] [data-executable="true"]')).not.toBeNull();
    await click(skill);
    await click(button("Use template"));
    const form = dialog().querySelector('[data-testid="template-start-form"]')!;
    expect(form).not.toBeNull();
    const details = form.querySelector<HTMLDetailsElement>("details")!;
    expect(form.querySelectorAll("details")).toHaveLength(1);
    expect(details.open).toBe(false);
    expect(button("Details")).toBeUndefined();
    expect(form.querySelector('[role="group"][aria-label="Workers on demand"]')?.closest("details")).toBe(details);
    expect(form.querySelector('section[aria-label="Brings"]')?.closest("details")).toBe(details);
    expect(form.querySelector('[data-testid="template-identity"]')?.closest("details")).toBe(details);
    await click(details.querySelector("summary")!);
    expect(details.open).toBe(true);
    expect(details.querySelector("h2")).toBeNull();
    expect(details.textContent).not.toContain(startablePreview.form.template.description);
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.getAttribute("data-tone")).toBe("neutral");
    expect(start().disabled).toBe(true);
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    expect(start().disabled).toBe(false);
    await click(start());
    const input = fixture.actions.createOrchestrationPair.mock.calls[0][0] as { template: unknown };
    expect(input.template).toEqual({
      template: { name: "com.example/research-desk", version: "2.1.0" },
      inputs: { question: "EU heat pumps", depth: "standard" },
      opt_out: ["skill:source-check"],
      decision_id: "tcd_7f3a2c",
      secret_bindings: { SEARCH_API_KEY: "SEARCH_API_KEY" },
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("shows a manager start failure as the dialog's start error and stays open", async () => {
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, consent: null });
    fixture.actions.createOrchestrationPair.mockRejectedValue(new Error(`/api/orchestration/new → HTTP 400: ${JSON.stringify(templateStartResultFixtures[2])}`));
    await renderPicker();
    await chooseResearchDesk();
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    await click(start());
    const error = dialog().querySelector('[data-testid="picker-start-error"]')!;
    expect(error.getAttribute("role")).toBe("alert");
    expect(error.textContent).toContain("codex is not signed in");
    expect(error.textContent).toContain("Use claude:sonnet");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("renders the package icon as an image, never inline SVG, with a fallback when absent", async () => {
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, consent: null });
    await renderPicker();
    expect(dialog().querySelector('[data-testid="template-header"] [data-template-icon="fallback"]')).not.toBeNull();
    await chooseResearchDesk();
    const icon = dialog().querySelector<HTMLImageElement>('[data-testid="template-header"] [data-template-icon] img, [data-testid="template-header"] img[data-template-icon]')!;
    expect(icon.getAttribute("src")).toBe(templatePreviewFixture.form.template.icon?.data_url);
    expect(dialog().querySelector('[data-testid="template-header"] svg')).toBeNull();
  });

  it("shows only real asks and the missing harness that blocks a local start", async () => {
    fixture.templates.preview.mockResolvedValue({ ...templatePreviewFixture, consent: null, asks: [{ kind: "shell" }] });
    await renderPicker();
    await chooseResearchDesk();
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.getAttribute("data-tone")).toBe("neutral");
    expect(start().disabled).toBe(true);
    expect(buttons().filter((candidate) => candidate.textContent?.trim() === "Run on another machine")).toHaveLength(1);
    const asks = Array.from(dialog().querySelectorAll("[data-ask]")).map((row) => row.getAttribute("data-ask"));
    expect(asks).toEqual(["shell"]);
    expect(dialog().querySelector('[data-testid="template-where-issue"]')?.textContent).toMatch(/codex isn't installed on this machine/i);
    const issue = dialog().querySelector('[data-testid="template-where-issue"]')!;
    expect(issue.closest("details")).toBeNull();
    const details = dialog().querySelector<HTMLDetailsElement>("details")!;
    expect(details.open).toBe(false);
    expect(dialog().querySelector('[data-ask="shell"]')?.closest("details")).toBe(details);
    await click(button("Run on another machine"));
    expect(details.open).toBe(true);
    expect(dialog().querySelector('[role="group"][aria-label="Placement"]')).not.toBeNull();
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.getAttribute("data-tone")).toBe("red");
  });

  it.each([false, true])("uses only host-reported missing-tool readiness (reported: %s)", async (reported) => {
    const tool = "mcp:weaverqa-missing-tool";
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, consent: null, asks: [{ kind: "tool", role: "researcher", tool }],
      where_needed: reported ? { needs: [{ kind: "missing_tool", tool }] } : null });
    await renderPicker();
    await chooseResearchDesk();
    const issue = dialog().querySelector('[data-testid="template-where-issue"]');
    if (reported) {
      expect(issue?.textContent).toContain(`${tool} isn't available`);
      expect(issue?.closest("details")).toBeNull();
      expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.textContent).toContain(tool);
    } else {
      expect(issue).toBeNull();
      expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.textContent).not.toContain(tool);
    }
    expect(dialog().querySelector('[data-ask="tool"]')?.closest("details")).not.toBeNull();
  });

  it("blocks start until a required secret is bound when the vault has no entry of that name", async () => {
    fixture.vault.mockResolvedValueOnce([]);
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, consent: null });
    await renderPicker();
    await chooseResearchDesk();
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.textContent).toBe("Choose a secret for SEARCH_API_KEY.");
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.getAttribute("data-tone")).toBe("amber");
    expect(start().disabled).toBe(true);
    const secret = dialog().querySelector('[aria-label^="Secret for SEARCH_API_KEY"]');
    expect(secret).not.toBeNull();
    expect(secret?.closest("details")).toBeNull();
    expect(dialog().querySelector<HTMLDetailsElement>("details")?.open).toBe(false);
  });

  it("keeps default workers in one collapsed section and returns from instructions to the form", async () => {
    const defaultPreview = {
      ...startablePreview, consent: null, asks: [], payload: [], form: { ...startablePreview.form, inputs: [], pickers: [],
        template: templateRecallFixture.fallback,
        roles: [
          { kind: "manager" as const, role: "orchestrator", label: "orchestrator", authority: "builtin" as const, start: "with_team" as const },
          { kind: "manager" as const, role: "planner", label: "planner", authority: "builtin" as const, start: "with_team" as const },
          { kind: "manager" as const, role: "cartographer", label: "cartographer", authority: "builtin" as const, start: "on_demand" as const },
          { kind: "manager" as const, role: "curator", label: "curator", authority: "builtin" as const, start: { schedule: "45m..2h" } },
          { kind: "worker" as const, role: "qa", label: "qa", authority: "none" as const, start: "on_demand" as const },
        ] },
    };
    fixture.templates.preview.mockImplementation(async (template: { name: string }) => template.name === "dev.domios/default" ? defaultPreview : startablePreview);
    await renderPicker();
    const roster = dialog().querySelector('[data-testid="pair-affordance"]')!;
    const rows = () => Array.from(roster.querySelectorAll('[role="listitem"]')).map((row) => row.getAttribute("data-pair-role"));
    expect(rows()).toEqual(["orchestrator", "planner", "later"]);
    const later = button("Joins later (2)")!;
    expect(later.getAttribute("aria-expanded")).toBe("false");
    await click(later);
    expect(rows()).toEqual(["orchestrator", "planner", "later", "cartographer", "curator"]);
    expect(roster.textContent).toContain("Joins when the Orchestrator needs it");
    // An on-demand manager with no model shows the Orchestrator's current pick, never a blank row.
    expect(roster.querySelector('[data-pair-role="cartographer"] button[aria-label^="Cartographer provider"]')?.getAttribute("aria-label")).toBe("Cartographer provider: Claude Code");
    expect(dialog().querySelector('[role="group"][aria-label="Workers on demand"]')?.textContent).toContain("QA");
    expect(dialog().querySelector('[data-testid="template-where"]')).not.toBeNull();
    expect(button("Details")).toBeUndefined();
    const details = dialog().querySelector<HTMLDetailsElement>("details")!;
    expect(details.open).toBe(false);
    expect(dialog().querySelector('[role="group"][aria-label="Workers on demand"]')?.closest("details")).toBe(details);
    await click(details.querySelector("summary")!);
    expect(details.open).toBe(true);
    fixture.templates.file.mockResolvedValue({ path: "roles/orchestrator.md", text: "Instructions" });
    await click(buttons().find((candidate) => candidate.textContent?.startsWith("Read the instructions for")));
    expect(dialog().querySelector('[data-testid="template-instructions"]')).not.toBeNull();
    await click(button("Back to Default team"));
    expect(dialog().querySelector('[data-testid="pair-affordance"]')).not.toBeNull();
    await click(start());
    expect(fixture.actions.createOrchestrationPair.mock.calls[0][0]).not.toHaveProperty("template");
  });

  it("starts the default through the template start once the owner picks a later manager's model", async () => {
    const defaultPreview = {
      ...startablePreview, consent: null, asks: [], payload: [], where_needed: null, form: { ...startablePreview.form, inputs: [], pickers: [],
        template: templateRecallFixture.fallback,
        roles: [{ kind: "manager" as const, role: "cartographer", label: "cartographer", authority: "builtin" as const, start: "on_demand" as const }] },
    };
    fixture.templates.preview.mockImplementation(async () => defaultPreview);
    await renderPicker();
    expect(dialog().querySelector('button[aria-label^="Cartographer provider"]')).toBeNull();
    await click(button("Joins later (1)"));
    const provider = dialog().querySelector<HTMLButtonElement>('button[aria-label^="Cartographer provider"]')!;
    expect(provider.disabled).toBe(false);
    expect(dialog().querySelector('[aria-label^="Cartographer model"]')).not.toBeNull();
    // Showing the Orchestrator's pick is not a choice; only the owner's pick moves the start to the template.
    await choose("Cartographer provider", "Claude Code");
    await click(start());
    const input = fixture.actions.createOrchestrationPair.mock.calls[0][0] as { template: { template: unknown; manager_models: Record<string, { model: string }> } };
    expect(input.template.template).toEqual({ name: templateRecallFixture.fallback.name, version: templateRecallFixture.fallback.version });
    expect(Object.keys(input.template.manager_models)).toEqual(["cartographer"]);
    expect(input.template.manager_models.cartographer.model.startsWith("claude:")).toBe(true);
  });

  it("keeps a same-as manager on the Orchestrator's model until the owner chooses one, and can return to it", async () => {
    const sameAs = modelDecisionFixtures.find((decision) => decision.reason === "same_as")!;
    const defaultPreview = {
      ...startablePreview, consent: null, asks: [], payload: [], where_needed: null, form: { ...startablePreview.form, inputs: [],
        template: templateRecallFixture.fallback, pickers: [{ role: "curator", label: "curator", authority: "builtin" as const, decision: sameAs }],
        roles: [{ kind: "manager" as const, role: "curator", label: "curator", authority: "builtin" as const, start: { schedule: "45m..2h" }, same_as: "orchestrator" }] },
    };
    fixture.templates.preview.mockImplementation(async () => defaultPreview);
    await renderPicker();
    await click(button("Joins later (1)"));
    expect(dialog().querySelector('[aria-label^="Curator model"]')?.getAttribute("aria-label")).toMatch(/^Curator model: Same as Orchestrator · /);
    expect(dialog().querySelector('[aria-label^="Curator provider"]')).toBeNull();
    await choose("Curator model", "Choose a model…");
    expect(dialog().querySelector('[aria-label^="Curator provider"]')).not.toBeNull();
    await click(button("Use the Orchestrator's model"));
    expect(dialog().querySelector('[aria-label^="Curator provider"]')).toBeNull();
    await click(start());
    expect(fixture.actions.createOrchestrationPair.mock.calls[0][0]).not.toHaveProperty("template");
  });

  it("names a later manager's same_as from its role and lets a declared model win over the Orchestrator prefill", async () => {
    const declared = { choice: { model: "claude:sonnet" }, reason: "first_preference" as const, amber: false };
    const undecided = { reason: "needs_choice" as const, amber: false };
    const defaultPreview = {
      ...startablePreview, consent: null, asks: [], payload: [], where_needed: null, form: { ...startablePreview.form, inputs: [], pickers: [],
        template: templateRecallFixture.fallback,
        roles: [
          { kind: "manager" as const, role: "cartographer", label: "cartographer", authority: "builtin" as const, start: "on_demand" as const, model: declared },
          { kind: "manager" as const, role: "curator", label: "curator", authority: "builtin" as const, start: { schedule: "45m..2h" }, model: undecided, same_as: "orchestrator" },
        ] },
    };
    fixture.templates.preview.mockImplementation(async () => defaultPreview);
    await renderPicker();
    await click(button("Joins later (2)"));
    expect(dialog().querySelector('[aria-label^="Curator model"]')?.getAttribute("aria-label")).toMatch(/^Curator model: Same as Orchestrator · /);
    expect(dialog().querySelector('[aria-label^="Curator provider"]')).toBeNull();
    expect(dialog().querySelector('[aria-label^="Cartographer model"]')?.getAttribute("aria-label")).toContain("sonnet");
    fixture.templates.preview.mockResolvedValue({ ...defaultPreview, consent: startablePreview.consent,
      form: { ...defaultPreview.form, template: startablePreview.form.template } });
    await chooseResearchDesk();
    const managers = dialog().querySelector('[data-testid="template-review-consent"] section[aria-label="Managers"]')!.textContent;
    expect(managers).toContain("Same as Orchestrator");
    expect(managers).toContain("sonnet");
  });

  it("gives every with_team manager its own picker by role name, places a custom manager among the managers and implies no Planner", async () => {
    const undecided = { reason: "needs_choice" as const, amber: false };
    const customPreview = {
      ...startablePreview, consent: null, form: { ...startablePreview.form, inputs: [],
        pickers: [
          { role: "orchestrator", label: "Lead", authority: "builtin" as const, decision: undecided },
          { role: "security_lead", label: "Security lead", authority: "none" as const, decision: undecided },
        ],
        roles: [
          { kind: "manager" as const, role: "orchestrator", label: "Lead", authority: "builtin" as const, start: "with_team" as const },
          { kind: "manager" as const, role: "security_lead", label: "Security lead", authority: "none" as const, start: "with_team" as const, description: "Reviews threat models." },
          { kind: "worker" as const, role: "researcher", label: "researcher", authority: "none" as const, start: "on_demand" as const, description: "Researches one angle." },
          { kind: "worker" as const, role: "writer", label: "writer", authority: "none" as const, start: "on_demand" as const },
        ] },
    };
    fixture.templates.preview.mockResolvedValue(customPreview);
    await renderPicker();
    await chooseResearchDesk();
    const roster = dialog().querySelector('[data-testid="template-managers"]')!;
    const rows = Array.from(roster.querySelectorAll('[role="listitem"]')).map((row) => row.getAttribute("data-pair-role"));
    expect(rows).toEqual(["orchestrator", "security_lead"]);
    expect(dialog().querySelector('[aria-label^="Planner"]')).toBeNull();
    expect(roster.querySelector('[data-pair-role="security_lead"] [data-authority="none"]')).not.toBeNull();
    expect(roster.querySelector('[data-pair-role="orchestrator"] [data-authority]')).toBeNull();
    expect(dialog().querySelectorAll('[role="group"][aria-label="Workers on demand"] [aria-label="Workers"] li')).toHaveLength(2);
    await choose("Security lead provider", "Claude Code");
    await click(start());
    const input = fixture.actions.createOrchestrationPair.mock.calls[0][0] as { template: { manager_models: Record<string, { model: string }> } };
    expect(Object.keys(input.template.manager_models)).toEqual(["security_lead"]);
    expect(input.template.manager_models.security_lead.model.startsWith("claude:")).toBe(true);
  });

  it("groups the review by kind with the custom manager among the managers", async () => {
    const customPreview = {
      ...startablePreview, form: { ...startablePreview.form,
        roles: [
          { kind: "worker" as const, role: "researcher", label: "researcher", authority: "none" as const, start: "on_demand" as const },
          { kind: "manager" as const, role: "security_lead", label: "Security lead", authority: "none" as const, start: "on_demand" as const },
          { kind: "manager" as const, role: "orchestrator", label: "Lead", authority: "builtin" as const, start: "with_team" as const },
        ] },
    };
    fixture.templates.preview.mockResolvedValue(customPreview);
    await renderPicker();
    await chooseResearchDesk();
    const review = dialog().querySelector('[data-testid="template-review-consent"]')!;
    const kinds = (section: string) => Array.from(review.querySelectorAll(`section[aria-label="${section}"] [data-role-kind]`)).map((row) => row.getAttribute("data-role-kind"));
    expect(kinds("Managers")).toEqual(["manager", "manager"]);
    expect(kinds("Workers on demand")).toEqual(["worker"]);
    expect(review.querySelectorAll('section[aria-label="Managers"] [data-authority="none"]')).toHaveLength(1);
    expect(review.querySelector('section[aria-label="Workers on demand"] [data-authority]')).toBeNull();
  });

  it("keeps card identity concise and its full description available on hover", async () => {
    await renderPicker();
    await click(button("Change template"));
    const card = buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"))!;
    expect(card.textContent).not.toContain("2.1.0");
    expect(card.textContent).not.toContain("unverified");
    expect(card.title).toBe(templateListFixture.templates.find((summary) => summary.name === "com.example/research-desk")?.description);
    expect(button("Use template")).toBeUndefined();
    expect(button("Use Research desk")).toBeUndefined();
  });

  it("allows selection only after host clearance, including when reopening the gallery", async () => {
    let clearResearch: (() => void) | undefined;
    fixture.templates.preview.mockImplementation((template: { name: string }, options?: { signal?: AbortSignal }) => {
      if (template.name === "com.example/research-desk" && options?.signal) {
        return new Promise((resolve) => { clearResearch = () => resolve(startablePreview); });
      }
      return Promise.resolve(startablePreview);
    });
    await renderPicker();
    await click(button("Change template"));
    const row = buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"))!;
    expect(row.getAttribute("aria-disabled")).toBe("true");
    expect(row.title).toBe("Checking…");
    const previewCalls = fixture.templates.preview.mock.calls.length;
    await click(row);
    await act(async () => row.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Enter", bubbles: true })));
    expect(fixture.templates.preview.mock.calls).toHaveLength(previewCalls);
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).toBeNull();
    await act(async () => clearResearch!());
    expect(row.getAttribute("aria-disabled")).toBe("false");
    await click(button("Back to the Team form"));
    await click(button("Change template"));
    const research = () => buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"))!;
    expect(research().getAttribute("aria-disabled")).toBe("true");
    const abandonedClearance = clearResearch;
    await click(button("Back to the Team form"));
    await click(button("Change template"));
    await act(async () => abandonedClearance!());
    expect(research().getAttribute("aria-disabled")).toBe("true");
    await act(async () => clearResearch!());
    expect(research().getAttribute("aria-disabled")).toBe("false");
  });

  it("keeps an unavailable card inert and exposes its reason", async () => {
    fixture.templates.preview.mockRejectedValue(new Error("template_unavailable: offline package"));
    await renderPicker();
    await click(button("Change template"));
    const card = buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"))!;
    expect(card.getAttribute("aria-disabled")).toBe("true");
    expect(card.title).toContain("offline package");
    expect(document.getElementById(card.getAttribute("aria-describedby")!)?.textContent).toContain("offline package");
    fixture.templates.preview.mockClear();
    await click(card);
    await act(async () => card.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Enter", bubbles: true })));
    expect(fixture.templates.preview).not.toHaveBeenCalled();
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
    expect(dialog().querySelector('[data-testid="template-gallery"]')).not.toBeNull();
  });

  it("shows a typed schedule refusal from the preview as a blocked start with its reason", async () => {
    const refused = templateStartResultFixtures.find((result) => result.status === "refused" && result.refusal.code === "schedule_unsupported_role")!;
    fixture.templates.preview.mockImplementation(async (template: { name: string }) => {
      if (template.name === "dev.domios/default") throw new Error("no default preview");
      throw new Error(`/api/templates/preview → HTTP 422: ${JSON.stringify(refused)}`);
    });
    await renderPicker();
    await click(button("Change template"));
    expect(button("Use Research desk")).toBeUndefined();
    const refusedCard = buttons().find((candidate) => candidate.getAttribute("aria-pressed") === "false" && candidate.textContent?.includes("Research desk"));
    expect(refusedCard?.closest("li")?.textContent).toContain("Can't start");
    expect(refusedCard?.getAttribute("aria-label")).toContain("Security lead");
    expect(refusedCard?.title).toContain("Security lead");
    const descriptionId = refusedCard?.getAttribute("aria-describedby");
    expect(descriptionId).toBeTruthy();
    expect(document.getElementById(descriptionId!)?.textContent).toContain("Security lead");
    await click(button("Back to the Team form"));
    await click(button("Change template"));
    expect(button("Use Research desk")).toBeUndefined();
    const row = buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"));
    const scroll = vi.spyOn(window.HTMLElement.prototype, "scrollIntoView").mockImplementation(() => undefined);
    try {
      await click(row);
      expect(scroll).toHaveBeenCalledWith({ block: "nearest", behavior: "instant" });
      expect(scroll.mock.contexts).toContain(row?.closest("li"));
    } finally {
      scroll.mockRestore();
    }
    const selectedReason = Array.from(dialog().querySelectorAll('[role="alert"]')).find((alert) => alert.getAttribute("aria-label")?.includes("Research desk"));
    expect(selectedReason?.textContent).toContain("Security lead");
    expect(row?.closest("li")?.contains(selectedReason!)).toBe(true);
    expect(row?.closest("li")?.getAttribute("data-selected")).toBe("true");
    await click(buttons().find((candidate) => candidate.getAttribute("aria-pressed") === "false" && candidate.textContent?.includes("Release train")));
    expect(row?.closest("li")?.querySelector('[role="alert"]')).toBeNull();
    expect(row?.closest("li")?.getAttribute("data-selected")).toBe("false");
    await click(row);
    expect(dialog().querySelector('[data-testid="template-start-form"]')).toBeNull();
    expect(button("Use Research desk")).toBeUndefined();
    const card = buttons().find((candidate) => candidate.getAttribute("aria-pressed") === "true" && candidate.textContent?.includes("Research desk"));
    expect(card?.closest("li")?.hasAttribute("data-blocked")).toBe(true);
    expect(button("Use template")).toBeUndefined();
    fixture.templates.preview.mockClear();
    await click(card);
    await act(async () => card!.dispatchEvent(new window.KeyboardEvent("keydown", { code: "Space", bubbles: true })));
    expect(fixture.templates.preview).not.toHaveBeenCalled();
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
  });

  it("uses a gallery card in one action and reads role instructions inside the same dialog", async () => {
    fixture.templates.file.mockResolvedValue({ path: "managers/lead.md", text: "# Lead\n\nPlan the research." });
    await renderPicker();
    await click(button("Change template"));
    await click(buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk")));
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).not.toBeNull();
    const read = buttons().find((candidate) => candidate.textContent?.startsWith("Read the instructions for"));
    await click(read);
    const step = dialog().querySelector('[data-testid="template-instructions"]')!;
    expect(step).not.toBeNull();
    expect(fixture.templates.file).toHaveBeenCalledWith(expect.objectContaining({ template: { name: "com.example/research-desk", version: "2.1.0" } }), expect.anything());
    expect(step.querySelector("h1")?.textContent).toBe("Lead");
    expect(dialog().querySelector('[data-testid="picker-confirm"]')).toBeNull();
    await click(button("Back to Research desk"));
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).not.toBeNull();
  });

  it.each(["Enter", "Space"])("takes a Built-in straight to its form on %s, still carrying its consent decision", async (code) => {
    const builtin = { ...startablePreview.form.template, provenance: "builtin" as const, verified: true };
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, form: { ...startablePreview.form, template: builtin } });
    await renderPicker();
    await click(button("Change template"));
    const card = buttons().find((candidate) => candidate.getAttribute("aria-pressed") !== null && candidate.textContent?.includes("Research desk"))!;
    await act(async () => card.dispatchEvent(new window.KeyboardEvent("keydown", { code, bubbles: true })));
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).toBeNull();
    expect(dialog().querySelector('[data-testid="template-start-form"]')).not.toBeNull();
    expect(document.activeElement).toBe(dialog().querySelector('input[placeholder="The market question to research"]'));
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    await click(start());
    const input = fixture.actions.createOrchestrationPair.mock.calls[0][0] as { template: { decision_id?: string } };
    expect(input.template.decision_id).toBe(startablePreview.consent?.decision_id);
  });

  it("keeps package actions without a second Details view", async () => {
    await renderPicker();
    await click(button("Change template"));
    const kebab = button("More actions for Research desk");
    await act(async () => { kebab!.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true })); });
    const items = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')).map((item) => item.textContent?.trim());
    expect(items).not.toContain("Details");
    expect(items).toContain("Export…");
    expect(items).toContain("Share to Neighborhood…");
    expect(items).toContain("Delete");
  });

  it("deletes a template from My templates only after an inline confirm", async () => {
    fixture.templates.delete.mockResolvedValue(exchange.delete_result);
    await renderPicker();
    await click(button("Change template"));
    const kebab = button("More actions for Research desk");
    expect(kebab).toBeDefined();
    await act(async () => { kebab!.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true })); });
    const remove = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((item) => item.textContent?.trim() === "Delete");
    await click(remove);
    expect(fixture.templates.delete).not.toHaveBeenCalled();
    const confirm = dialog().querySelector('ul[aria-label="Templates"] [role="group"][aria-label="Delete Research desk"]')!;
    expect(confirm.textContent).toContain("Delete Research desk?");
    expect(button("More actions for Research desk")).toBeUndefined();
    await click(Array.from(confirm.querySelectorAll<HTMLButtonElement>("button")).find((candidate) => candidate.textContent?.trim() === "Delete"));
    expect(fixture.templates.delete).toHaveBeenCalledWith({ name: "com.example/research-desk", version: "2.1.0" });
    expect(fixture.templates.list).toHaveBeenCalledTimes(2);
  });

  it("opens a shared package straight into its review with the update it brings", async () => {
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "single", providerId: "claude" }));
    const update = { ...exchange.import_result.update, name: "com.example/research-desk", from_version: "2.0.3", to_version: "2.1.0" } as TemplateUpdateDiff;
    openTemplatePreview({ id: "workspace", cwd: "/workspace" }, { name: "com.example/research-desk", version: "2.1.0" }, update);
    await renderPicker();
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).not.toBeNull();
    const diff = dialog().querySelector('[data-testid="template-update-diff"]')?.textContent;
    expect(diff).toContain("Roles added: Editor");
    expect(diff).toContain("No longer brings: Market sizes older than two years mislead");
    expect(diff).not.toContain("memory:1");
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
  });

  it("imports a package file the OS opened and stops at its consent review", async () => {
    localStorage.setItem("codeterm:new-session-choice", JSON.stringify({ mode: "single", providerId: "claude" }));
    fixture.templates.import.mockResolvedValue({ ...exchange.import_result, update: null });
    openTemplateFileReview({ id: "workspace", cwd: "/workspace" }, "/downloads/research-desk.team");
    await renderPicker();
    expect(fixture.templates.import).toHaveBeenCalledExactlyOnceWith("/downloads/research-desk.team");
    expect(fixture.templates.preview).toHaveBeenCalledWith({ name: exchange.import_result.template.name, version: exchange.import_result.template.version });
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).not.toBeNull();
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
  });

  it("reads a script inside the package before the owner keeps it", async () => {
    const payload = (templatePreviewFixture.payload ?? []).map((item) => item.id === "skill:source-check" ? { ...item, script_path: "source-check/scripts/fetch.py" } : item);
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, payload });
    fixture.templates.file.mockResolvedValue({ path: "source-check/scripts/fetch.py", text: "print('fetch')" });
    await renderPicker();
    await chooseResearchDesk();
    await click(button("View source-check/scripts/fetch.py"));
    expect(fixture.templates.file).toHaveBeenCalledWith({ template: { name: "com.example/research-desk", version: "2.1.0" }, path: "source-check/scripts/fetch.py" }, expect.anything());
    const viewer = Array.from(document.body.querySelectorAll('[role="dialog"]')).pop()!;
    expect(viewer.querySelector('[aria-label="source-check/scripts/fetch.py"]')?.textContent).toBe("print('fetch')");
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
  });

  it("runs a chosen worker type on a Full-access peer through the same pair request", async () => {
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, consent: null });
    await renderPicker();
    await chooseResearchDesk();
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    await click(dialog().querySelector<HTMLElement>("details > summary")!);
    await click(button("Run on another machine"));
    expect(dialog().querySelector('[aria-label^="Machine for Lead"]')).toBeNull();
    await choose("Machine for Researcher", "eight");
    await click(start());
    const input = fixture.actions.createOrchestrationPair.mock.calls[0][0] as { template: { role_placements?: Record<string, string> } };
    expect(input.template.role_placements).toEqual({ researcher: "eight" });
  });

  it("asks a peer to start the whole team and never starts it here", async () => {
    fixture.templates.preview.mockResolvedValue({ ...startablePreview, consent: null });
    fixture.templates.peerStart.mockResolvedValue(exchange.peer_start_accepted);
    await renderPicker();
    await chooseResearchDesk();
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    await click(dialog().querySelector<HTMLElement>("details > summary")!);
    await click(button("Run on another machine"));
    await choose("Machine for this team", "eight");
    await click(start());
    expect(fixture.actions.createOrchestrationPair).not.toHaveBeenCalled();
    const request = fixture.templates.peerStart.mock.calls[0][0] as { peer: string; start: { template: { template: unknown; role_placements?: unknown } } };
    expect(request.peer).toBe("eight");
    expect(request.start.template.template).toEqual({ name: "com.example/research-desk", version: "2.1.0" });
    expect(request.start.template).not.toHaveProperty("role_placements");
    expect(fixture.actions.upsertToast).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });
});
