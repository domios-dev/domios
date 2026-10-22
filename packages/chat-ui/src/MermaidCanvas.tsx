import { useMemo } from "react";
import { mermaidSvgSize } from "./mermaidFullscreen";
import { ZoomCanvas } from "./ZoomCanvas";

export function MermaidCanvas({ svg }: { svg: string }) {
  const bounds = useMemo(() => ({ x: 0, y: 0, ...(mermaidSvgSize(svg) ?? { width: 800, height: 600 }) }), [svg]);
  return <ZoomCanvas bounds={bounds}>
    <div style={{ width: bounds.width, height: bounds.height }} className="[&>svg]:block [&>svg]:h-full [&>svg]:w-full [&>svg]:max-w-none" dangerouslySetInnerHTML={{ __html: svg }} />
  </ZoomCanvas>;
}
