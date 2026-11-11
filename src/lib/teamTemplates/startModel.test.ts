import { describe, expect, it } from "vitest";
import {
  modelDecisionFixtures, templateListFixture, templatePreviewFixture, templateRecallFixture, templateStartResultFixtures, templateUnavailableFixture,
} from "../transport/teamTemplates.testdata";
import {
  bringsSummary, buildStartRequest, decisionNotice, effortNote, exportFormats, galleryEntries, pickerSeed, roleTiming, startBlocker, startErrorView,
} from "./startModel";

const [firstPreference, sameAs, normalDefault, , unknownModel, absentHarness, notSignedIn, quota, sameAsUnavailable, noReasoningEffort, missingTool, exhausted] = modelDecisionFixtures;

describe("template start model", () => {
  it("prefills decided managers and leaves undecided ones to the owner", () => {
    expect(pickerSeed(firstPreference)).toEqual({ providerId: "claude", modelId: "opus", effortId: undefined });
    expect(pickerSeed(sameAs)).toEqual({ providerId: "claude", modelId: "opus", effortId: undefined });
    expect(pickerSeed(normalDefault)).toBeNull();
    const lead = templatePreviewFixture.form.pickers[0].decision;
    expect(pickerSeed(lead)).toEqual({ providerId: "codex", modelId: "gpt-6.1-sol", effortId: "high" });
  });

  it("keeps the amber within-list fallback and the quiet effort note", () => {
    const lead = templatePreviewFixture.form.pickers[0].decision;
    expect(decisionNotice(lead)?.tone).toBe("amber");
    expect(effortNote(lead)).toBe("codex:gpt-6.1-sol supports up to high");
    expect(decisionNotice(firstPreference)).toBeNull();
    expect(effortNote(firstPreference)).toBeNull();
  });

  it("explains each blocker with its recovery choices", () => {
    expect(decisionNotice(absentHarness)?.actions).toEqual([{ kind: "install", harness: "codex" }]);
    expect(decisionNotice(notSignedIn)?.actions).toEqual([{ kind: "sign_in", harness: "gemini" }]);
    expect(decisionNotice(quota)?.actions).toEqual([{ kind: "substitute", model: "codex:gpt-6.1-sol" }, { kind: "wait" }]);
    expect(decisionNotice(unknownModel)?.actions).toEqual([
      { kind: "substitute", model: "claude:opus" }, { kind: "substitute", model: "claude:opus-4.5" },
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
  });

  it("counts only the payload the owner keeps", () => {
    const payload = templatePreviewFixture.payload ?? [];
    expect(bringsSummary(payload, new Set())).toBe("1 document, 1 memory fact, 1 skill, 1 automation");
    expect(bringsSummary(payload, new Set(["memory:0", "skill:source-check"]))).toBe("1 document, 1 automation");
  });

  it("blocks start on a missing input, then on a missing harness", () => {
    expect(startBlocker(templatePreviewFixture, { question: "", depth: "standard" })).toBe("Fill in question");
    expect(startBlocker(templatePreviewFixture, { question: "EU heat pumps" })).toBe("Needs codex on this machine");
    expect(startBlocker({ ...templatePreviewFixture, where_needed: null }, { question: "EU heat pumps" })).toBeNull();
  });

  it("carries the owner decision id, opt-outs and filled inputs on the start request", () => {
    const request = buildStartRequest({
      preview: templatePreviewFixture,
      inputs: { question: "EU heat pumps", depth: "" },
      optOut: new Set(["skill:source-check", "memory:0"]),
      managerModels: {},
    });
    expect(request).toEqual({
      template: { name: "com.example/research-desk", version: "2.1.0" },
      inputs: { question: "EU heat pumps" },
      opt_out: ["memory:0", "skill:source-check"],
      decision_id: "tcd_7f3a2c",
    });
    expect(buildStartRequest({ preview: { ...templatePreviewFixture, consent: null }, inputs: {}, optOut: new Set(), managerModels: {} }))
      .not.toHaveProperty("decision_id");
  });

  it("shows a manager start failure with its reasons and choices, and other refusals plainly", () => {
    const [, needsChoice, managerFailed, consent, missingInputs, notFound] = templateStartResultFixtures;
    expect(startErrorView(new Error(`/api/orchestration/new → HTTP 400: ${JSON.stringify(managerFailed)}`))).toEqual({
      message: "The Planner couldn't start", reasons: ["codex is not signed in"], choices: ["Sign in to codex", "Use claude:sonnet"],
    });
    expect(startErrorView(JSON.stringify(needsChoice)).reasons).toEqual(["Orchestrator · Lead"]);
    expect(startErrorView(JSON.stringify(consent)).reasons).toEqual(["new_grants"]);
    expect(startErrorView(JSON.stringify(missingInputs)).reasons).toEqual(["question"]);
    expect(startErrorView(JSON.stringify(notFound)).message).toBe("dev.example/missing isn't available");
    expect(startErrorView(new Error(`template_unavailable: ${templateUnavailableFixture.message}`)).message).toBe(templateUnavailableFixture.message);
    expect(startErrorView(new Error(JSON.stringify({ error: `template_unavailable: ${templateUnavailableFixture.message}` }))).message)
      .toBe(templateUnavailableFixture.message);
  });

  it("lists the best recalled match first and keeps the default available", () => {
    const fallback = templateRecallFixture.fallback;
    const names = (entries: { name: string }[]) => entries.map((entry) => entry.name);
    expect(names(galleryEntries(templateListFixture.templates, templateRecallFixture, fallback, "all", "")))
      .toEqual(["dev.domios/release-train", "dev.domios/default", "com.example/research-desk"]);
    expect(names(galleryEntries(templateListFixture.templates, null, fallback, "all", ""))).toEqual(["dev.domios/default", "com.example/research-desk"]);
    expect(names(galleryEntries(templateListFixture.templates, templateRecallFixture, fallback, "mine", ""))).toEqual(["com.example/research-desk"]);
    expect(names(galleryEntries(templateListFixture.templates, templateRecallFixture, fallback, "all", "market"))).toEqual(["com.example/research-desk"]);
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
