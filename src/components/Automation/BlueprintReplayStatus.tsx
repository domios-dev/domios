import type { BlueprintReplayOutcome } from "./blueprintReplayOutcome";
import { runTime } from "./RunLog";

export function BlueprintReplayStatus({ done, at, outcome }: { done: boolean; at: number; outcome: BlueprintReplayOutcome | null }) {
  return <span role="status" aria-live="polite" className="bp-replay-status" data-failed={done && outcome?.failed || undefined}>
    {done ? outcome?.label ?? `Replayed run ${runTime(at)}` : `Replaying run ${runTime(at)}`}
  </span>;
}
