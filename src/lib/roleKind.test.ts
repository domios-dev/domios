import { describe, expect, it } from "vitest";
import { roleKindLabel } from "./roleKind";

describe("roleKindLabel", () => {
  it("prefers the Team-registered label over the kind and the structural role", () => {
    expect(roleKindLabel({ role: "Manager", role_profile: "orchestrator", role_label: "Lead" })).toBe("Lead");
    expect(roleKindLabel({ role: "Worker", role_profile: "researcher", role_label: "Researcher" })).toBe("Researcher");
  });

  it("falls back to the kind, then the structural role", () => {
    expect(roleKindLabel({ role: "Manager", role_profile: "planner", role_label: " " })).toBe("Planner");
    expect(roleKindLabel({ role: "Worker", role_profile: "researcher" })).toBe("Worker");
    expect(roleKindLabel({ role: null, role_label: "Lead" })).toBeNull();
  });
});
