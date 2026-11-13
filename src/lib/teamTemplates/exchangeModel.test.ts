import { describe, expect, it } from "vitest";
import exchange from "../../../src-tauri/crates/codeterm-team-package/fixtures/contracts/exchange.json";
import { templateListFixture, templatePreviewFixture } from "../transport/teamTemplates.testdata";
import {
  authorLine, authorRows, cardByline, distillName, initialSecretBindings, isTemplateFileName, linkRows, shareAsksSummary, updateDiffLines, usedByLine,
} from "./exchangeModel";
import { hostPeersOf } from "./hostPeers";

type TemplateAsk = import("../../../gen/TemplateAsk").TemplateAsk;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;

describe("template exchange model", () => {
  it("names authors and follows only web links from a shared package", () => {
    const summary = { authors: [
      { name: "Jane Doe", role: "maintainer" as const, url: "https://example.com/jane" },
      { name: "Example Labs", url: "javascript:alert(1)" },
    ], links: { homepage: "https://example.com/desk", repository: "file:///etc/passwd", docs: "http://docs.example.com/" } };
    expect(authorLine({ ...summary, provenance: "imported" })).toBe("by Jane Doe");
    const [, desk] = templateListFixture.templates;
    expect(cardByline({ ...desk, ...summary, provenance: "imported" })).toBe("by Jane Doe · Researcher, Checker, Writer, 3 more");
    expect(cardByline({ ...desk, ...summary, provenance: "saved" })).toBe("by you · Researcher, Checker, Writer, 3 more");
    expect(cardByline({ provenance: "builtin", workers: 1, worker_names: ["build_owner"], skills: 1, skill_names: ["source-check"] })).toBe("Build owner, source-check");
    expect(cardByline({ provenance: "builtin", workers: 0, skills: 2, skill_names: ["source-check", "market-sizing"] })).toBe("source-check, market-sizing");
    expect(cardByline({ provenance: "builtin", workers: 0 })).toBeNull();
    expect(authorRows(summary)).toEqual([
      { name: "Jane Doe", role: "maintainer", url: "https://example.com/jane" },
      { name: "Example Labs", role: null, url: null },
    ]);
    expect(linkRows(summary)).toEqual([{ label: "Homepage", url: "https://example.com/desk" }, { label: "Docs", url: "http://docs.example.com/" }]);
    expect(authorLine({ authors: [], provenance: "imported" })).toBeNull();
  });

  it("says which roles use an item", () => {
    const [brief] = exchange.distill_result.includes;
    expect(usedByLine(brief)).toBe("used by Orchestrator, Researcher");
    expect(usedByLine(brief, [{ role: "orchestrator", label: "Lead" }, { role: "researcher", label: "researcher" }])).toBe("used by Lead, Researcher");
    expect(usedByLine({ used_by: ["build_owner"] })).toBe("used by Build owner");
    expect(usedByLine({ used_by: [] })).toBeNull();
  });

  it("summarises a shared package's asks in one line", () => {
    expect(shareAsksSummary(exchange.share_card.asks as TemplateAsk[])).toBe("Asks for 1 secret. Runs a script on start.");
    expect(shareAsksSummary(templatePreviewFixture.asks ?? [])).toBe(
      "Asks for network for api.example.com, *.wikipedia.org, edits to the project, shell commands, mcp:io.github.example/search and 1 secret. Runs a script on start.");
    expect(shareAsksSummary([])).toBeNull();
  });

  it("lists what an update changes", () => {
    expect(updateDiffLines(exchange.import_result.update as TemplateUpdateDiff)).toEqual([
      "Roles added: Editor", "Roles changed: Checker", "Kind changed: Checker (worker → manager)", "Brings now: citations", "No longer brings: Market sizes older than two years mislead",
      "New asks: network for api.example.com",
    ]);
  });

  it("binds a required secret to the vault entry of the same name and skips optional ones", () => {
    const asks: TemplateAsk[] = [
      { kind: "secret", name: "SEARCH_API_KEY", purpose: "search", required: true },
      { kind: "secret", name: "SLACK_TOKEN", purpose: "notify", required: false },
      { kind: "secret", name: "GH_TOKEN", purpose: "push", required: true },
    ];
    expect(initialSecretBindings(asks, ["SEARCH_API_KEY", "SLACK_TOKEN"])).toEqual({ SEARCH_API_KEY: "SEARCH_API_KEY" });
  });

  it("recognises team package files and names a bake under the owner's namespace", () => {
    expect(["desk.team", "desk.team.yaml", "desk.TEAM.yml"].map(isTemplateFileName)).toEqual([true, true, true]);
    expect(["team.zip", "notes.yaml", "desk.teams"].map(isTemplateFileName)).toEqual([false, false, false]);
    expect(distillName("Video studio!")).toBe("me/video-studio");
    expect(distillName("  ")).toBe("me/team");
  });

  it("offers only online peers with Full access as hosts", () => {
    expect(hostPeersOf([
      { instance_id: "b", instance_name: "eight", online: true, access_level: "full" },
      { instance_id: "a", instance_name: "air", online: true, access_level: "full" },
      { instance_id: "c", instance_name: "mini", online: false, access_level: "full" },
      { instance_id: "d", instance_name: "guest", online: true, access_level: "read_only" },
    ])).toEqual([{ id: "a", name: "air" }, { id: "b", name: "eight" }]);
  });
});
