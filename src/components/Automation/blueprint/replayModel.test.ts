import { describe, expect, it } from "vitest";
import { blueprintNodeKey as key } from "../blueprintExpansion";
import { viewerPipeline } from "../blueprintViewer.testdata";
import { blueprintRunEvents, buildBlueprintGraph, type BlueprintRunEvent } from "./graphModel";
import { projectRunPath, replayStep } from "./replayModel";
import { pipelineModel } from "../pipelineModel";
import capturedWatch from "../blueprintWatchCaptured.testdata.json";
import type { AutomationGraph } from "../types";
import { watchRuns, withRetainedRuns } from "./runEvidence";

function fixture() {
  const pipeline = viewerPipeline().runs![0].pipeline;
  const build = buildBlueprintGraph(pipeline, new Set([key([], "invoke")]));
  let result = build.next();
  while (!result.done) result = build.next();
  return { pipeline, model: result.value, events: blueprintRunEvents(pipeline) };
}

describe("Blueprint historical path", () => {
  it("leaves Latest at full opacity independently of node state", () => {
    const { model, events } = fixture();
    const latest = projectRunPath(model, events, false);
    expect(latest.dim).toBe(false);
    expect(latest.model).toBe(model);
    expect(latest.active).toBeNull();
  });

  it("restores reached nodes and edges, including isolated evidence and node outcomes", () => {
    const { model, events } = fixture();
    const path = projectRunPath(model, events, true);
    expect(path.dim).toBe(true);
    expect(path.nodes.has(key([], "probe"))).toBe(true);
    expect(path.nodes.has(key([], "invoke"))).toBe(true);
    expect(path.nodes.has(key(["invoke"], "return-count"))).toBe(true);
    expect(path.nodes.has(key([], "skipped"))).toBe(false);
    expect([...path.wires].some((id) => model.wires.find((wire) => wire.id === id)?.to.node === key([], "invoke"))).toBe(true);
    expect(path.model.nodes.find((node) => node.id === "invoke")?.evidence?.state).toBe("failed");
    expect(model.wires.filter((wire) => wire.to.node === key([], "skipped")).some((wire) => path.wires.has(wire.id))).toBe(false);
    const isolated = projectRunPath(model, events.slice(0, 1), true);
    expect(isolated.nodes.has(key([], "probe"))).toBe(true);
  });

  it("steps chronologically, hides future evidence, and highlights the current node", () => {
    const { model, events } = fixture();
    expect(events.map((event) => event.key)).toEqual([key([], "probe"), key([], "invoke"), key(["invoke"], "return-count")]);
    const start = projectRunPath(model, events, true, 0);
    expect(start.nodes.size).toBe(0);
    expect(start.active).toBeNull();
    const middle = projectRunPath(model, events, true, 1);
    expect(middle.active).toBe(key([], "probe"));
    expect(middle.model.nodes.find((node) => node.id === "invoke")?.evidence?.state).toBe("not reached");
    expect(middle.wires.size).toBe(1);
    expect(projectRunPath(model, events, true, 3).active).toBe(key(["invoke"], "return-count"));
  });

  it("uses journal IDs for ordering and excludes foreign runs and definitions", () => {
    const { pipeline } = fixture();
    const events = blueprintRunEvents({ ...pipeline, graph_definition_version: 2,
      nested_run: { id: pipeline.runId!, definition_version: 2, nodes: [
        { entry_id: 3, step_index: 0, step_name: "probe", state: "done", outcome: "done", definition_version: 2 },
        { entry_id: 1, step_index: 0, step_name: "probe", state: "running", definition_version: 2 },
        { entry_id: 2, step_index: 0, step_name: "probe", state: "done", run_id: "foreign", definition_version: 2 },
        { entry_id: 4, step_index: 0, step_name: "probe", state: "done", definition_version: 1 },
      ] } });
    expect(events.map((event) => event.evidence.state)).toEqual(["running", "done"]);
    expect(events[1].evidence.outcome).toBe("done");
  });

  it("replays retained watch evidence in source, decision and handler order", () => {
    const graph = capturedWatch as AutomationGraph;
    const snapshot = graph.blueprints![0];
    const pipeline = withRetainedRuns(pipelineModel(snapshot), watchRuns(graph, snapshot)).runs![0].pipeline;
    const events = blueprintRunEvents(pipeline);
    expect(events.map((event) => event.key)).toEqual([key([], "source"), key([], "decide"), key([], "notify")]);
    expect(events[1].evidence.outcome).toBe("passed");
  });

  it("highlights the enclosing call when its active child is collapsed", () => {
    const { model, events } = fixture();
    const collapsed = { ...model, nodes: model.nodes.filter((node) => node.callPath.length === 0) };
    expect(projectRunPath(collapsed, events, true).active).toBe(key([], "invoke"));
  });

  it("retains routes from earlier visits when a node executes more than once", () => {
    const { model } = fixture();
    const events: BlueprintRunEvent[] = [
      { key: key([], "probe"), evidence: { state: "done", outcome: "done", summary: null, at: 1 } },
      { key: key([], "invoke"), evidence: { state: "done", outcome: "done", summary: null, at: 2 } },
      { key: key([], "probe"), evidence: { state: "failed", outcome: "failed", summary: null, at: 3 } },
    ];
    const path = projectRunPath(model, events, true);
    const route = model.wires.find((wire) => wire.kind === "exec" && wire.from.node === key([], "probe") && wire.to.node === key([], "invoke"))!;
    expect(path.wires.has(route.id)).toBe(true);
    expect(path.model.nodes.find((node) => node.id === "probe")?.evidence?.outcome).toBe("failed");
  });

  it("never dims a run whose per-node evidence is unavailable", () => {
    const { model } = fixture();
    expect(projectRunPath({ ...model, mode: "unavailable" }, [], true).dim).toBe(false);
    expect(projectRunPath(model, [], true).dim).toBe(false);
  });
});

describe("Replay stepping", () => {
  it("restarts, pauses, seeks and stops at the final entry", () => {
    let state = replayStep({ step: 3, playing: false }, { type: "play" }, 3);
    expect(state).toEqual({ step: 0, playing: true });
    state = replayStep(state, { type: "advance" }, 3);
    expect(state).toEqual({ step: 1, playing: true });
    state = replayStep(state, { type: "pause" }, 3);
    expect(replayStep(state, { type: "advance" }, 3)).toEqual({ step: 1, playing: false });
    expect(replayStep(state, { type: "seek", step: 2 }, 3)).toEqual({ step: 2, playing: false });
    expect(replayStep({ step: 2, playing: true }, { type: "advance" }, 3)).toEqual({ step: 3, playing: false });
  });

  it("bounds seek positions and refuses playback without entries", () => {
    expect(replayStep({ step: 1, playing: false }, { type: "seek", step: -4 }, 3).step).toBe(0);
    expect(replayStep({ step: 1, playing: true }, { type: "seek", step: 20 }, 3)).toEqual({ step: 3, playing: false });
    expect(replayStep({ step: 0, playing: false }, { type: "play" }, 0)).toEqual({ step: 0, playing: false });
  });
});
