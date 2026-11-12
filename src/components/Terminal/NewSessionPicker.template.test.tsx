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
  await click(button("Use template"));
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
  });

  it("reviews an unverified package before use, then starts it with the decision id, opt-outs and inputs", async () => {
    await renderPicker();
    await chooseResearchDesk();
    expect(dialog().querySelector('[data-testid="template-review-consent"]')).not.toBeNull();
    expect(start()).toBeNull();
    const skill = dialog().querySelector<HTMLButtonElement>('[role="switch"][aria-label="Bring source-check"]')!;
    expect(skill.getAttribute("aria-checked")).toBe("true");
    expect(dialog().querySelector('[data-payload-item="skill:source-check"] [data-executable="true"]')).not.toBeNull();
    await click(skill);
    await click(button("Use template"));
    const form = dialog().querySelector('[data-testid="template-start-form"]')!;
    expect(form).not.toBeNull();
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.getAttribute("data-tone")).toBe("amber");
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
    const asks = Array.from(dialog().querySelectorAll("[data-ask]")).map((row) => row.getAttribute("data-ask"));
    expect(asks).toEqual(["shell"]);
    expect(dialog().querySelector('[data-testid="template-where"] [data-testid="template-where-issue"]')?.textContent).toMatch(/codex isn't installed on this machine/i);
    await type(dialog().querySelector<HTMLInputElement>('input[placeholder="The market question to research"]')!, "EU heat pumps");
    expect(dialog().querySelector('[data-testid="picker-start-blocker"]')?.getAttribute("data-tone")).toBe("red");
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
    expect(dialog().querySelector('[aria-label^="Secret for SEARCH_API_KEY"]')).not.toBeNull();
  });

  it("shows the whole default team on the default form, and Details returns to that form", async () => {
    const defaultPreview = {
      ...startablePreview, consent: null, asks: [], payload: [], form: { ...startablePreview.form, inputs: [], pickers: [],
        template: templateRecallFixture.fallback,
        roles: [
          { group: "manager" as const, role: "orchestrator", label: "orchestrator", start: "with_team" as const },
          { group: "manager" as const, role: "planner", label: "planner", start: "with_team" as const },
          { group: "manager" as const, role: "cartographer", label: "cartographer", start: "on_demand" as const },
          { group: "manager" as const, role: "curator", label: "curator", start: { schedule: "45m..2h" } },
          { group: "worker" as const, role: "qa", label: "qa", start: "on_demand" as const },
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
    await click(button("Details"));
    expect(dialog().querySelector('[data-testid="template-review-detail"]')?.textContent).toContain("Planner");
    await click(button("Back to the Team form"));
    expect(dialog().querySelector('[data-testid="pair-affordance"]')).not.toBeNull();
    await click(start());
    expect(fixture.actions.createOrchestrationPair.mock.calls[0][0]).not.toHaveProperty("template");
  });

  it("starts the default through the template start once the owner picks a later manager's model", async () => {
    const defaultPreview = {
      ...startablePreview, consent: null, asks: [], payload: [], where_needed: null, form: { ...startablePreview.form, inputs: [], pickers: [],
        template: templateRecallFixture.fallback,
        roles: [{ group: "manager" as const, role: "cartographer", label: "cartographer", start: "on_demand" as const }] },
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
        template: templateRecallFixture.fallback, pickers: [{ slot: "curator" as const, label: "curator", decision: sameAs }],
        roles: [{ group: "manager" as const, role: "curator", label: "curator", start: { schedule: "45m..2h" } }] },
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

  it("uses a gallery card in one action and reads role instructions inside the same dialog", async () => {
    fixture.templates.file.mockResolvedValue({ path: "managers/lead.md", text: "# Lead\n\nPlan the research." });
    await renderPicker();
    await click(button("Change template"));
    await click(button("Use Research desk"));
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

  it("returns from a gallery row's Details to the gallery", async () => {
    await renderPicker();
    await click(button("Change template"));
    const kebab = button("More actions for Research desk");
    await act(async () => { kebab!.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true, cancelable: true })); });
    await click(Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((item) => item.textContent?.trim() === "Details"));
    expect(dialog().querySelector('[data-testid="template-review-detail"]')).not.toBeNull();
    await click(button("Back to templates"));
    expect(dialog().querySelector('[data-testid="template-gallery"]')).not.toBeNull();
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
    const confirm = dialog().querySelector('[role="alertdialog"]')!;
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
    expect(dialog().querySelector('[data-testid="template-update-diff"]')?.textContent).toContain("Roles added: Editor");
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
    await click(button("Run some roles on another machine"));
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
    await click(button("Run some roles on another machine"));
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
