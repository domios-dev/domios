import { useEffect, useState } from "react";
import { buildBlueprintGraph, type BlueprintGraphModel } from "./blueprintGraphModel";
import { buildIncrementally } from "./blueprintIncremental";
import type { BlueprintExpansion } from "./blueprintExpansion";
import { layoutPipelineChain, type PipelineChainLayout } from "./pipelineLayout";
import type { PipelineView } from "./pipelineModel";

function yieldToHost(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => { channel.port1.close(); channel.port2.close(); resolve(); };
    channel.port2.postMessage(null);
  });
}
export function useBlueprintGraph(pipeline: PipelineView, expanded: BlueprintExpansion) {
  const [result, setResult] = useState<{ source: PipelineView; expanded: BlueprintExpansion; model: BlueprintGraphModel; layout: PipelineChainLayout } | null>(null);
  const [failure, setFailure] = useState<{ source: PipelineView; error: string } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void buildIncrementally(buildBlueprintGraph(pipeline, expanded), yieldToHost, controller.signal).then((model) => {
      if (!controller.signal.aborted) setResult({ source: pipeline, expanded, model, layout: layoutPipelineChain(model.nodes, model.edges) });
    }).catch((error: unknown) => {
      if (!controller.signal.aborted) setFailure({ source: pipeline, error: String(error) });
    });
    return () => controller.abort();
  }, [pipeline, expanded]);
  return { graph: result?.source === pipeline && result.expanded === expanded ? result : null, error: failure?.source === pipeline ? failure.error : null };
}
