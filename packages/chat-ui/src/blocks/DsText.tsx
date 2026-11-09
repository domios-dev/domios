import { Fragment, useContext, useId, useMemo, type ReactNode } from "react";
import { DsSegmentView } from "./DsSegmentView";
import { groupImageRuns } from "./groupImageRuns";
import { useLocalArtifactTabId } from "../LocalArtifactScope";
import { DsRenderContext } from "./DsRenderContext";
import { useDsSegments } from "./useDsSegments";

export function DsText({ text, renderText }: { text: string; renderText: (source: string) => ReactNode }) {
  const scope = useContext(DsRenderContext);
  const instance = useId();
  const tab = useLocalArtifactTabId();
  const scanned = useDsSegments(text, { id: `${scope.authority}:${tab ?? ""}:${instance}`, trusted: scope.trusted }, scope.streaming, scope.cancelled);
  const segments = useMemo(() => groupImageRuns(scanned), [scanned]);
  return <>{segments.map((segment) => {
    if (segment.type === "markdown") return <Fragment key={segment.id}>{scope.trusted ? renderText(segment.text) : segment.text}</Fragment>;
    return <DsSegmentView key={segment.id} segment={segment} />;
  })}</>;
}
