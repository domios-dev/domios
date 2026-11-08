import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";
import { Button } from "../shared/Button";

/** Replaces the "Starting terminal…" spinner when the output subscription cannot deliver this terminal's first frame. */
export function TerminalStartFailure({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="absolute inset-0 z-20" data-testid="terminal-start-failure">
      <BrandState
        model={brandStateModel({ error: { title: "Terminal output did not arrive", sentence: "The session is still running. Retry reconnects its output." } })}
        className="absolute inset-0 ct-loading-material"
      >
        <Button variant="primary" onClick={(event) => { event.stopPropagation(); onRetry(); }}>Retry</Button>
      </BrandState>
    </div>
  );
}
