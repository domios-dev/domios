import { describe, expect, it } from "vitest";
import {
  modelDecisionFixtures, templateListFixture, templatePreviewFixture, templateRecallFixture, templateStartResultFixtures, templateUnavailableFixture,
} from "../transport/teamTemplates.testdata";
import {
  blockerView, bringsSummary, buildStartRequest, decisionNotice, effortLabel, effortNote, exportFormats, galleryEntries, groupKeptState, humanizePackageName,
  lacksAuthority, payloadItemText, pickerSeed, skillScriptPath, roleTiming, scrubRows, startBlocker, startErrorView, startRefusalView, managerRoles, rowDecisionNotice,
  consentOrigin, templateSource, templateTitle, templateUpdateFact,
  visiblePayload, withKept,
} from "./startModel";

const [firstPreference, sameAs, normalDefault, , unknownModel, absentHarness, notSignedIn, quota, sameAsUnavailable, noReasoningEffort, missingTool, exhausted] = modelDecisionFixtures;

describe("template start model", () => {
  it("prefills decided managers and leaves undecided ones to the owner", () => {
    expect(pickerSeed(firstPreference)).toEqual({ providerId: "harness-a", modelId: "opus", effortId: undefined });
    expect(pickerSeed(sameAs)).toEqual({ providerId: "harness-a", modelId: "opus", effortId: undefined });
    expect(pickerSeed(normalDefault)).toBeNull();
    const lead = templatePreviewFixture.form.pickers[0].decision;
    expect(pickerSeed(lead)).toEqual({ providerId: "harness-b", modelId: "gpt-6.1-sol", effortId: "high" });
  });

  it("keeps the amber within-list fallback and the quiet effort note", () => {
    const lead = templatePreviewFixture.form.pickers[0].decision;
    expect(decisionNotice(lead)?.tone).toBe("amber");
    expect(effortNote(lead)).toBe("harness-b:gpt-6.1-sol supports up to high");
    expect(decisionNotice(firstPreference)).toBeNull();
    expect(effortNote(firstPreference)).toBeNull();
  });

  it("explains each blocker with its recovery choices", () => {
    expect(decisionNotice(absentHarness)?.actions).toEqual([{ kind: "install", harness: "harness-b" }]);
    expect(decisionNotice(notSignedIn)?.actions).toEqual([{ kind: "sign_in", harness: "harness-c" }]);
    expect(decisionNotice(quota)?.actions).toEqual([{ kind: "substitute", model: "harness-b:gpt-6.1-sol" }, { kind: "wait" }]);
    expect(decisionNotice(unknownModel)?.actions).toEqual([
      { kind: "substitute", model: "harness-a:opus" }, { kind: "substitute", model: "harness-a:opus-4.5" },
    ]);
    expect(decisionNotice(sameAsUnavailable)?.tone).toBe("blocked");
    expect(decisionNotice(exhausted)?.tone).toBe("blocked");
    expect(decisionNotice(noReasoningEffort)).toMatchObject({ tone: "blocked", actions: [] });
    expect(decisionNotice(missingTool)?.actions).toEqual([{ kind: "connect_tool", tool: "mcp:github" }]);
  });

  it("says when non-starting roles join", () => {
    expect(roleTiming("with_team")).toBeNull();
    expect(roleTiming("on_demand")).toBe("Joins when the Orchestrator needs it");
    expect(roleTiming({ schedule: "45m..2h" })).toBe("Visits every 45 min to 2 h");
    expect(roleTiming({ schedule: "90m..1h" })).toBe("Visits every 60 to 90 min");
    expect(roleTiming({ schedule: "1h..2h" })).toBe("Visits every 1 to 2 h");
  });

  it("names templates by title, else by the humanized last id segment, and capitalizes efforts", () => {
    expect(templateTitle({ name: "dev.weaverqa/secret-probe", title: null })).toBe("Secret probe");
    expect(templateTitle({ name: "me/release_train", title: "Release train" })).toBe("Release train");
    expect(humanizePackageName("me/qa-desk")).toBe("QA desk");
    expect(effortLabel("medium")).toBe("Medium");
  });

  it("keeps the icon out of Brings and reads documents by their description", () => {
    const icon = { id: "file:icon.svg", kind: "file" as const, label: "icon.svg", executable: false };
    const doc = { id: "file:docs/how-we-research.md", kind: "file" as const, label: "docs/how-we-research.md", executable: false };
    expect(visiblePayload([icon, doc])).toEqual([doc]);
    expect(payloadItemText(doc)).toEqual({ name: "How we research", detail: "docs/how-we-research.md" });
    expect(payloadItemText({ ...doc, description: "How we research a market" }).name).toBe("How we research a market");
    expect(payloadItemText({ kind: "memory", label: "Prefer filings", memory_kind: "rule" })).toEqual({ name: "Prefer filings", detail: "Rule" });
    expect(payloadItemText({ kind: "automation", label: "stale-task-nudge", description: "Nudges the lead" })).toEqual({ name: "Nudges the lead", detail: null });
    expect(payloadItemText({ kind: "automation", label: "stale-task-nudge" }).name).toBe("Stale task nudge");
    const skill = { id: "skill:source-check", label: "source-check", script_path: "source-check/scripts/fetch.py" };
    expect(skillScriptPath(skill)).toBe("scripts/fetch.py");
    expect(skillScriptPath({ id: "skill:skills/sizing", label: "sizing", script_path: "skills/sizing/bin/run" })).toBe("bin/run");
  });

  it("selects or clears a whole group of payload items at once", () => {
    const items = [{ id: "a" }, { id: "b" }];
    expect(groupKeptState(items, new Set())).toBe("all");
    expect(groupKeptState(items, new Set(["a"]))).toBe("some");
    const cleared = withKept(new Set(["c"]), ["a", "b"], false);
    expect(groupKeptState(items, cleared)).toBe("none");
    expect([...withKept(cleared, ["a", "b"], true)]).toEqual(["c"]);
  });

  it("reports only what scrubbing removed, and chats without a count", () => {
    const none = { machines_removed: 0, paths_generalized: 0, secret_values_removed: 0, secret_asks_added: 0, history_removed: 0 };
    expect(scrubRows(none)).toEqual([]);
    const rows = scrubRows({ ...none, paths_generalized: 14, history_removed: 3, secret_values_removed: 1, secret_names: ["GH_TOKEN"] });
    expect(rows.map((row) => [row.label, row.count])).toEqual([["Paths", 14], ["Secret values", 1], ["Chats and history", null]]);
  });

  it("counts only the payload the owner keeps", () => {
    const payload = templatePreviewFixture.payload ?? [];
    expect(bringsSummary(payload, new Set())).toBe("1 document, 1 memory fact, 1 skill, 1 automation");
    expect(bringsSummary(payload, new Set(["memory:0", "skill:source-check"]))).toBe("1 document, 1 automation");
  });

  it("lists every blocker of a start in one line, by field label", () => {
    const none = { inputs: [], secrets: [], harnesses: [], tools: [], workspaces: [] };
    const noWhere = { ...templatePreviewFixture, where_needed: null };
    expect(startBlocker(noWhere, { question: "", depth: "standard" })).toEqual({ ...none, inputs: ["question"], secrets: ["SEARCH_API_KEY"] });
    const bound = { SEARCH_API_KEY: "research-desk/search" };
    const harness = startBlocker(templatePreviewFixture, { question: "EU heat pumps" }, bound);
    expect(harness).toEqual({ ...none, harnesses: ["harness-b"], tools: ["mcp:io.github.example/search"] });
    expect(startBlocker(noWhere, { question: "EU heat pumps" }, bound)).toBeNull();
    const onEight = { peer: "eight", workspaceChosen: true };
    expect(startBlocker(templatePreviewFixture, { question: "EU heat pumps" }, {}, onEight)).toBeNull();
    expect(startBlocker(templatePreviewFixture, { question: "" }, {}, onEight)).toEqual({ ...none, inputs: ["question"] });
    expect(startBlocker(templatePreviewFixture, { question: "EU heat pumps" }, {}, { peer: "eight", workspaceChosen: false }))
      .toEqual({ ...none, workspaces: ["eight"] });
    expect(blockerView({ ...none, workspaces: ["eight"] }, (id) => id)).toEqual({ tone: "amber", text: "Choose a workspace on eight." });
    const names = (id: string) => id === "harness-b" ? "Harness B" : id;
    expect(blockerView({ ...none, inputs: ["question"], secrets: ["SEARCH_API_KEY"] }, names))
      .toEqual({ tone: "amber", text: "Fill in Question and choose a secret for SEARCH_API_KEY." });
    expect(blockerView(harness!, names)).toEqual({ tone: "red", text: "Install Harness B and connect mcp:io.github.example/search to start." });
    expect(blockerView({ ...none, harnesses: ["harness-b"] }, names)).toEqual({ tone: "red", text: "Install Harness B to start." });
    expect(blockerView({ ...none, inputs: ["question"], harnesses: ["harness-b"] }, names).text).toBe("Fill in Question and install Harness B to start.");
  });

  it("carries the owner decision id, opt-outs and filled inputs on the start request", () => {
    const request = buildStartRequest({
      preview: templatePreviewFixture,
      inputs: { question: "EU heat pumps", depth: "" },
      optOut: new Set(["skill:source-check", "memory:0"]),
      managerModels: {},
      secretBindings: { SEARCH_API_KEY: "research-desk/search", OPTIONAL_KEY: "" },
      rolePlacements: { researcher: "eight", writer: "" },
    });
    expect(request).toEqual({
      template: { name: "com.example/research-desk", version: "2.1.0" },
      inputs: { question: "EU heat pumps" },
      opt_out: ["memory:0", "skill:source-check"],
      decision_id: "tcd_7f3a2c",
      secret_bindings: { SEARCH_API_KEY: "research-desk/search" },
      role_placements: { researcher: "eight" },
    });
    expect(buildStartRequest({ preview: { ...templatePreviewFixture, consent: null }, inputs: {}, optOut: new Set(), managerModels: {} }))
      .not.toHaveProperty("decision_id");
  });

  it("shows a manager start failure with its reasons and choices, and other refusals plainly", () => {
    const [, needsChoice, managerFailed, consent, missingInputs, notFound] = templateStartResultFixtures;
    expect(startErrorView(new Error(`/api/orchestration/new → HTTP 400: ${JSON.stringify(managerFailed)}`))).toEqual({
      message: "The Planner couldn't start", reasons: ["harness-b is not signed in"], choices: ["Sign in to harness-b", "Use harness-a:sonnet"],
    });
    expect(startErrorView(new Error(`template_start: ${JSON.stringify(managerFailed)}`)).message).toBe("The Planner couldn't start");
    expect(startErrorView(new Error(`HTTP 422: ${JSON.stringify({ code: "invalid_package", message: "notes.zip has no team.yaml", diagnostics: [{ message: "team.yaml is missing" }] })}`)))
      .toEqual({ message: "notes.zip has no team.yaml", reasons: ["team.yaml is missing"], choices: [] });
    expect(startErrorView(JSON.stringify(needsChoice)).reasons).toEqual(["Orchestrator · Lead"]);
    expect(startErrorView(JSON.stringify(consent)).reasons).toEqual(["new_grants"]);
    expect(startErrorView(JSON.stringify(missingInputs)).reasons).toEqual(["question"]);
    expect(startErrorView(JSON.stringify(notFound)).message).toBe("Missing isn't available");
    expect(startErrorView(`template_start: ${JSON.stringify(templateStartResultFixtures[6])}`).message).toBe("The Orchestrator is a manager and runs on this machine");
    expect(startErrorView(new Error(`template_unavailable: ${templateUnavailableFixture.message}`)).message).toBe(templateUnavailableFixture.message);
    expect(startErrorView(new Error(JSON.stringify({ error: `template_unavailable: ${templateUnavailableFixture.message}` }))).message)
      .toBe(templateUnavailableFixture.message);
  });

  it("lists the best recalled match first and keeps the default available", () => {
    const fallback = templateRecallFixture.fallback;
    const names = (entries: { name: string }[]) => entries.map((entry) => entry.name);
    expect(names(galleryEntries(templateListFixture.templates, templateRecallFixture, fallback, "all", "")))
      .toEqual(["dev.domios/video-studio", "dev.domios/default", "dev.domios/design-studio", "dev.domios/marketing-studio", "com.example/research-desk"]);
    expect(names(galleryEntries(templateListFixture.templates, null, fallback, "all", "")))
      .toEqual(["dev.domios/default", "dev.domios/video-studio", "dev.domios/design-studio", "dev.domios/marketing-studio", "com.example/research-desk"]);
    expect(names(galleryEntries(templateListFixture.templates, templateRecallFixture, fallback, "mine", ""))).toEqual(["com.example/research-desk"]);
    expect(names(galleryEntries(templateListFixture.templates, templateRecallFixture, fallback, "all", "market")))
      .toEqual(["dev.domios/marketing-studio", "com.example/research-desk"]);
  });

  it("keeps a harness blocker on the row only while that row runs on the blocked harness", () => {
    const cursor = { ...absentHarness, blocked: { kind: "absent_harness" as const, harness: "cursor" } };
    expect(rowDecisionNotice(cursor, "harness-b")).toBeNull();
    expect(rowDecisionNotice(cursor, "cursor")?.text).toBe("cursor isn't installed");
    expect(rowDecisionNotice(cursor, "")?.text).toBe("cursor isn't installed");
    expect(rowDecisionNotice(unknownModel, "harness-b")).toEqual(decisionNotice(unknownModel));
  });

  it("lists exactly the package's managers, built-in slots first, and never implies a Planner", () => {
    const security = { kind: "manager" as const, role: "security_lead", label: "Security lead", authority: "none" as const, start: "with_team" as const };
    const curator = { kind: "manager" as const, role: "curator", label: "curator", authority: "builtin" as const, start: { schedule: "45m..2h" } };
    const orchestrator = { kind: "manager" as const, role: "orchestrator", label: "Lead", authority: "builtin" as const, start: "with_team" as const };
    const worker = { kind: "worker" as const, role: "qa", label: "qa", authority: "none" as const, start: "on_demand" as const };
    expect(managerRoles([security, curator, worker, orchestrator]).map((role) => role.role)).toEqual(["orchestrator", "curator", "security_lead"]);
    expect([security, curator, worker, orchestrator].filter(lacksAuthority).map((role) => role.role)).toEqual(["security_lead"]);
  });

  it("reads a typed schedule refusal as a blocked start with its reason", () => {
    const refused = templateStartResultFixtures.find((result) => result.status === "refused" && result.refusal.code === "schedule_unsupported_role")!;
    const view = startRefusalView(new Error(`/api/templates/preview → HTTP 422: ${JSON.stringify(refused)}`));
    expect(view?.reasons).toHaveLength(1);
    expect(view?.reasons[0]).toContain("Security lead");
    expect(startRefusalView(new Error(JSON.stringify(templateStartResultFixtures[2])))).toBeNull();
    expect(startRefusalView(new Error("network down"))).toBeNull();
  });

  it("files an owner's import under Mine and a Neighborhood receipt under From friends, unverified until consented", () => {
    expect(templateSource({ provenance: "imported", verified: false })).toEqual({ filter: "mine", label: "From file · unverified", unverified: true, owned: true, exportable: true, deletable: true });
    expect(templateSource({ provenance: "imported", verified: true })).toEqual({ filter: "mine", label: "Mine", unverified: false, owned: true, exportable: true, deletable: true });
    expect(templateSource({ provenance: "imported", received_from: "Mira", verified: false })).toEqual({ filter: "friends", label: "From Mira · unverified", unverified: true, owned: false, exportable: false, deletable: true });
    expect(templateSource({ provenance: "imported", received_from: "Mira", verified: true })).toMatchObject({ filter: "friends", label: "From Mira", unverified: false });
    expect(templateSource({ provenance: "saved", verified: false })).toMatchObject({ filter: "mine", owned: true, exportable: true, unverified: false });
    expect(templateSource({ provenance: "builtin", verified: false })).toMatchObject({ filter: "builtin", owned: false, exportable: true });
    const imported = { name: "me/cli-import", version: "1.0.0", description: "", provenance: "imported" as const, workers: 0, verified: false };
    const received = { ...imported, name: "mira/desk", received_from: "Mira" };
    const names = (entries: { name: string }[]) => entries.map((entry) => entry.name);
    const fallback = templateRecallFixture.fallback;
    expect(names(galleryEntries([imported, received], null, fallback, "mine", ""))).toEqual(["me/cli-import"]);
    expect(names(galleryEntries([imported, received], null, fallback, "friends", ""))).toEqual(["mira/desk"]);
  });

  it("reads official channel teams as Built-in and community channel teams as their channel, unverified until consent", () => {
    const bundled = { provenance: "builtin" as const, version: "1.0.0", verified: true };
    const updated = { ...bundled, version: "1.2.0", channel: "domios-teams", bundled_version: "1.0.0" };
    expect(templateSource(updated)).toEqual(templateSource(bundled));
    expect(templateSource(updated)).toMatchObject({ filter: "builtin", unverified: false, deletable: false });
    expect(templateUpdateFact(bundled)).toBeNull();
    expect(templateUpdateFact({ ...updated, bundled_version: "1.2.0" })).toBeNull();
    expect(templateUpdateFact(updated)).toContain("1.2.0");
    expect(templateUpdateFact(updated)).toContain("1.0.0");

    const community = { provenance: "channel" as const, channel: "acme-id", verified: false };
    const unverified = templateSource(community, "Acme");
    expect(unverified).toMatchObject({ filter: "channels", unverified: true, owned: false, exportable: true, deletable: false });
    expect(unverified.label).toContain("Acme");
    expect(templateSource(community).label).toContain("acme-id");
    const consented = templateSource({ ...community, verified: true }, "Acme");
    expect(consented).toMatchObject({ filter: "channels", unverified: false, owned: false, deletable: false });
    expect(consented.label).toContain("Acme");
    expect(consentOrigin(community, "Acme")).toContain("Acme");

    const summary = { name: "acme/desk", version: "1.0.0", description: "", workers: 0, ...community };
    const fallback = templateRecallFixture.fallback;
    expect(galleryEntries([summary], null, fallback, "channels", "").map((entry) => entry.name)).toEqual(["acme/desk"]);
    expect(galleryEntries([summary], null, fallback, "mine", "")).toEqual([]);
  });

  it("offers the one-file export only when the package has no binary icon", () => {
    const svg = exportFormats(templatePreviewFixture.form.template);
    expect(svg.map((option) => [option.format, option.filename, Boolean(option.disabledReason)])).toEqual([
      ["yaml", "research-desk.team.yaml", false], ["team", "research-desk.team", false], ["folder", "research-desk/", false],
    ]);
    const png = exportFormats({ name: "com.example/research-desk", icon: { media_type: "image/png", data_url: "data:image/png;base64,AA==" } });
    expect(png[0].disabledReason).toBeTruthy();
  });
});
