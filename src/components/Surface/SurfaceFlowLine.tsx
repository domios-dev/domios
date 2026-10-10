import { useSurfacePulse } from "../../lib/surfaceActivity";
import { useSurfaceControlled } from "../../lib/surfaceControlStore";

/** 1px line that carries one liquid pass per agent verb; static while idle or under the user's control. */
export function SurfaceFlowLine({ surfaceTabId }: { surfaceTabId: string }) {
  const pulse = useSurfacePulse(surfaceTabId);
  const controlled = useSurfaceControlled(surfaceTabId);
  const state = controlled ? "controlled" : pulse ? "driving" : "idle";
  return (
    <div className="ct-surface-flow" data-testid="surface-flow-line" data-state={state} aria-hidden="true">
      {state === "driving" && pulse && <span key={pulse.nonce} className="ct-surface-flow__pass" data-testid="surface-flow-pass" />}
    </div>
  );
}
