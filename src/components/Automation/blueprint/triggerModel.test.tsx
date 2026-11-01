import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { BlueprintDocument } from "../../../lib/blueprintApi";
import { pipelineModel } from "../pipelineModel";
import type { AutomationGraph } from "../types";
import capturedWatch from "../blueprintWatchCaptured.testdata.json";
import { buildBlueprintGraph } from "./graphModel";
import { layoutBlueprint } from "./layout";
import { BlueprintNodeCard } from "./NodeCard";
import { BlueprintNodeDetails } from "./NodeDetails";
import { nodeTrigger, singleCadence, triggerRuntime } from "./triggerModel";

const document: BlueprintDocument = {
  schema_version: 2, kind: "watch", name: "PR reply",
  steps: [{ id: "source", uses: "probe", every: { lit: "2m" }, timeout: { lit: "24h" }, with: { command: { lit: "echo ready" } } }],
};

function project(definition = document, live = true) {
  const captured = capturedWatch as AutomationGraph;
  const machine = captured.nodes[0];
  const pipeline = { ...pipelineModel(captured.blueprints![0]), call_graph: definition, triggerRuntime: live ? triggerRuntime(machine) : null };
  const build = buildBlueprintGraph(pipeline, new Set());
  let next = build.next();
  while (!next.done) next = build.next();
  return next.value;
}

describe("Blueprint trigger projection", () => {
  it("projects a cadence on its emitting node with the machine schedule", () => {
    const model = project();
    const source = model.nodes.find((node) => node.id === "source")!;
    expect(source.trigger).toEqual({
      badge: "every 2m", label: "Trigger: every 2m", cadence: "2m", timeout: "24h",
      runtime: { nextAt: 1792823634720, lastAt: 1792821836666, expiresAt: 1792825434720 },
    });
    expect(model.nodes.filter((node) => node.trigger)).toHaveLength(1);
    expect(singleCadence(document)).toBe("every 2m");
  });

  it("recognizes cadence independently of capability and preserves dynamic values", () => {
    expect(nodeTrigger({ id: "custom", uses: "custom", every: { lit: "5m" } }, false)?.badge).toBe("every 5m");
    expect(nodeTrigger({ id: "custom", every: { input: "interval" } }, false)?.badge).toBe("every input: interval");
    expect(nodeTrigger({ id: "worker", uses: "probe" }, false)).toBeUndefined();
    expect(nodeTrigger({ id: "worker", uses: "cron" }, false)).toBeUndefined();
    expect(singleCadence({ ...document, steps: [...document.steps, { id: "second", every: { lit: "5m" } }] })).toBeNull();
  });

  it("projects explicit schedule, event, webhook and input trigger roles", () => {
    expect(nodeTrigger({ type: "schedule", every: { lit: "30m" } }, true)?.badge).toBe("every 30m");
    expect(nodeTrigger({ type: "webhook" }, true)?.badge).toBe("on webhook");
    expect(nodeTrigger({ type: "event", kind: { lit: "tab_output" } }, true)?.badge).toBe("on tab_output");
    expect(nodeTrigger({ type: "input" }, true)?.badge).toBe("on input");
    expect(triggerRuntime(undefined)).toBeNull();
  });

  it("keeps event emitters visible and avoids inventing runtime timestamps", () => {
    const event = project({ ...document, steps: [{ id: "events", uses: "observe" }] });
    expect(event.nodes.find((node) => node.id === "events")?.trigger?.badge).toBe("on event");
    const schedule = project({ ...document, kind: "pipeline", trigger: { type: "schedule", every: { lit: "10m" } }, steps: [] });
    expect(schedule.nodes.find((node) => node.control === "trigger")?.trigger?.badge).toBe("every 10m");
    const historical = project(document, false);
    expect(historical.nodes.find((node) => node.id === "source")?.trigger?.runtime).toBeNull();
    expect(historical.nodes.find((node) => node.id === "source")?.trigger?.badge).toBe("every 2m");
  });

  it("exposes an accessible badge and all trigger facts in the inspector", () => {
    const model = project();
    const source = model.nodes.find((node) => node.id === "source")!;
    const laid = layoutBlueprint(model).nodes.find((node) => node.node.id === "source")!;
    const host = window.document.createElement("div");
    host.innerHTML = renderToStaticMarkup(<><BlueprintNodeCard laid={laid} /><BlueprintNodeDetails node={source} model={model} expanded={false} onExpand={() => {}} onClose={() => {}} /></>);
    expect(host.querySelector('[aria-label="Trigger: every 2m"]')?.textContent).toContain("every 2m");
    const section = host.querySelector('section[aria-label="Trigger"]')!;
    const facts = [...section.querySelectorAll("dt")].map((term) => [term.textContent, term.nextElementSibling?.textContent]);
    expect(facts).toEqual([
      ["Cadence", "every 2m"], ["Timeout", "24h"],
      ["Expires", new Date(1792825434720).toLocaleString()],
      ["Next scheduled run", new Date(1792823634720).toLocaleString()],
      ["Last probe", new Date(1792821836666).toLocaleString()],
    ]);
  });
});
