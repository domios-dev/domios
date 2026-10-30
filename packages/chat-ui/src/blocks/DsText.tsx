import { Fragment, useContext, useId, type ReactNode } from "react";
import { DsSegmentView } from "./DsSegmentView";
import { useLocalArtifactTabId } from "../LocalArtifactScope";
import { DsRenderContext } from "./DsRenderContext";
import { useDsSegments } from "./useDsSegments";

export function DsText({ text, renderText }: { text: string; renderText: (source: string) => ReactNode }) {
  const scope = useContext(DsRenderContext);
  const instance = useId();
  const tab = useLocalArtifactTabId();
  const segments = useDsSegments(text, { id: `${scope.authority}:${tab ?? ""}:${instance}`, trusted: scope.trusted }, scope.streaming, scope.cancelled);
  return <>{segments.map((segment) => {
    if (segment.type === "markdown") return <Fragment key={segment.id}>{scope.trusted ? renderText(segment.text) : segment.text}</Fragment>;
    return <DsSegmentView key={segment.id} segment={segment} />;
  })}</>;
}
