import { describe, expect, it } from "vitest";
import { skippedView, teamChannelView } from "./teamChannels";

type SkipReason = import("../../../gen/SkipReason").SkipReason;
type TeamChannelRow = import("../../../gen/TeamChannelRow").TeamChannelRow;

const CODES: SkipReason[] = [
  "missing_package", "invalid_package", "requires_newer_domios", "reserved_name", "index_mismatch",
  "invalid_path", "invalid_index_entry", "duplicate_name", "host_policy", "other",
];

const row = (overrides: Partial<TeamChannelRow>): TeamChannelRow => ({
  name: "acme", displayName: "Acme", description: "", url: "u", path: "p", kind: "git", official: false, ...overrides,
});

describe("team channel view", () => {
  it("words every skip code without the host's raw detail and names the Domios a package needs", () => {
    const texts = CODES.map((code) => skippedView({ name: "x", code, reason: "opaque-detail" }));
    for (const view of texts) {
      expect(view.text.length).toBeGreaterThan(0);
      expect(view.text).not.toContain("opaque");
      expect(view.detail).toBe("opaque-detail");
    }
    expect(new Set(texts.map((view) => view.text)).size).toBe(CODES.length);
    expect(skippedView({ name: "x", code: "requires_newer_domios", reason: "r", requiredDomios: "9.9.9" }).text).toContain("9.9.9");
    const unknown = skippedView({ name: "x", code: "from_a_newer_host" as string as SkipReason, reason: "r" });
    expect(unknown.text).toBe(skippedView({ name: "x", code: "other", reason: "r" }).text);
  });

  it("keeps the official channel unremovable and calm when its refresh fails, and a community failure visible", () => {
    const official = teamChannelView(row({ name: "domios-teams", official: true, lastRefresh: "offline" }));
    expect(official).toMatchObject({ removable: false, autoUpdates: true, refresh: { kind: "bundled", reason: "offline" } });
    const community = teamChannelView(row({ lastRefresh: "offline" }));
    expect(community).toMatchObject({ removable: true, autoUpdates: false, refresh: { kind: "failed", reason: "offline" } });
    expect(teamChannelView(row({})).refresh).toEqual({ kind: "never" });
  });
});
