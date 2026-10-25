import { describe, expect, it } from "vitest";
import { probeStateLabel } from "./probeProgress";
import { sentinelStateLabel } from "../Jobs/JobRow";
import { sourceStage } from "./blueprintModel.testdata";

const stage = sourceStage("watch", { type: "probe" }).nodes[0];

describe("probe progress", () => {
  it("shows the current episode and repeat count", () => {
    const node = { ...stage, state: "active", detail: { probe_state: "waiting", runs_since_transition: 3, last_probe_at: 61000 } };
    expect(probeStateLabel(node)).toBe("waiting ×3");
    expect(probeStateLabel({ ...node, detail: { ...node.detail, probe_state: "error", runs_since_transition: 0 } })).toBe("error ×0");
    expect(probeStateLabel({ ...node, detail: { ...node.detail, probe_state: "other" } })).toBe("other ×3");
  });
  it("feeds the Jobs row from the current probe", () => {
    const graph = sourceStage("watch", { type: "probe" }, { state: "active", detail: { probe_state: "waiting", runs_since_transition: 3, last_probe_at: 61000 } });
    const root = graph.nodes[0];
    expect(sentinelStateLabel({ scenario: graph.scenarios[0], type: "Blueprint", root, state: "active" })).toBe("waiting ×3");
  });
  it("preserves stopped and paused status and reports unprobed nodes", () => {
    const node = { ...stage, detail: { probe_state: "waiting", runs_since_transition: 3 } };
    expect(probeStateLabel({ ...node, state: "stopped" })).toBe("stopped");
    expect(probeStateLabel({ ...node, state: "paused" })).toBe("paused");
    expect(probeStateLabel({ ...stage, state: "active", detail: undefined })).toBe("active");
  });
});
