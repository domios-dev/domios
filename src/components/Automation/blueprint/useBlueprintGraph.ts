import { useEffect, useState } from "react";
import { buildIncrementally } from "../blueprintIncremental";
import type { BlueprintExpansion } from "../blueprintExpansion";
import type { PipelineView } from "../pipelineModel";
import { buildBlueprintGraph, type BlueprintGraphModel } from "./graphModel";
import { layoutBlueprint, type BlueprintLayout, type CanvasNote } from "./layout";

function yieldToHost(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => { channel.port1.close(); channel.port2.close(); resolve(); };
    channel.port2.postMessage(null);
  });
}

/** Builds the typed graph in host-yielding chunks, so a large Blueprint never blocks input; the last graph stays up while the next one builds. */
export function useBlueprintGraph(pipeline: PipelineView, expanded: BlueprintExpansion, note: CanvasNote | null = null) {
  const [result, setResult] = useState<{ source: PipelineView; expanded: BlueprintExpansion; note: CanvasNote | null; model: BlueprintGraphModel; layout: BlueprintLayout } | null>(null);
  const [failure, setFailure] = useState<{ source: PipelineView; error: string } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void buildIncrementally(buildBlueprintGraph(pipeline, expanded), yieldToHost, controller.signal).then((model) => {
      if (!controller.signal.aborted) setResult({ source: pipeline, expanded, note, model, layout: layoutBlueprint(model, note) });
    }).catch((error: unknown) => {
      if (!controller.signal.aborted) setFailure({ source: pipeline, error: String(error) });
    });
    return () => controller.abort();
  }, [pipeline, expanded, note]);
  const current = result?.source === pipeline && result.expanded === expanded && result.note === note ? result : null;
  return { graph: current ?? result, building: !current, error: failure?.source === pipeline ? failure.error : null };
}
