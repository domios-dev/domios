import { describe, expect, it } from "vitest";
import { createTeamTemplatesTransport, withTemplate, type TeamTemplatesClient } from "./teamTemplates";
import { templateListFixture, templatePreviewFixture, templateStartRequestFixture } from "./teamTemplates.testdata";

function fixture(response: unknown) {
  const calls: { method: string; path: string; body: unknown }[] = [];
  const client: TeamTemplatesClient = {
    async json<T>(method: string, path: string, body?: unknown): Promise<T> {
      calls.push({ method, path, body });
      return response as T;
    },
  };
  return { calls, transport: createTeamTemplatesTransport(client) };
}

describe("team templates transport", () => {
  it("lists, previews and recalls through the shared contracts", async () => {
    const { calls, transport } = fixture(templateListFixture);
    expect(await transport.list()).toEqual(templateListFixture);
    await transport.preview({ name: "com.example/research-desk", version: "2.1.0" });
    await transport.recall("ship a release");
    expect(calls).toEqual([
      { method: "GET", path: "/api/team-templates", body: undefined },
      { method: "POST", path: "/api/team-templates/preview", body: { template: { name: "com.example/research-desk", version: "2.1.0" } } },
      { method: "POST", path: "/api/team-templates/recall", body: { query: "ship a release" } },
    ]);
  });

  it("lists and changes team channels through the tagged channel action", async () => {
    const { calls, transport } = fixture({ channels: [] });
    await transport.channels();
    await transport.channelAction({ action: "add", url: "https://example.invalid/acme.git", name: null });
    await transport.channelAction({ action: "refresh", name: "acme" });
    await transport.channelAction({ action: "remove", name: "acme" });
    expect(calls).toEqual([
      { method: "GET", path: "/api/team-channels", body: undefined },
      { method: "POST", path: "/api/team-channels", body: { action: "add", url: "https://example.invalid/acme.git", name: null } },
      { method: "POST", path: "/api/team-channels", body: { action: "refresh", name: "acme" } },
      { method: "POST", path: "/api/team-channels", body: { action: "remove", name: "acme" } },
    ]);
  });

  it("carries a template only when one is chosen, so the default pair is unchanged", () => {
    const pair = { orchestrator_agent: "claude", planner_agent: "claude" };
    expect(withTemplate(pair)).toEqual(pair);
    expect(withTemplate(pair, templateStartRequestFixture)).toEqual({ ...pair, template: templateStartRequestFixture });
  });

  it("exposes preview fixtures with payload opt-out ids and executable flags", () => {
    const executable = templatePreviewFixture.payload?.filter((item) => item.executable).map((item) => item.id);
    expect(executable).toEqual(["skill:source-check"]);
    expect(templatePreviewFixture.consent?.decision_id).toBe("tcd_7f3a2c");
  });
});
