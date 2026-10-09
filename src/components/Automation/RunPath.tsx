import { NodeKindBadge } from "./NodeKindBadge";
import { useMemo } from "react";
import type { GraphRun } from "../../lib/blueprintRun";
import { useBlueprintReplay } from "./useBlueprintReplay";
import { runPathSteps } from "./runPathModel";

export function RunPath({ run }: { run: GraphRun }) {
  const path = useMemo(() => runPathSteps(run), [run]);
  const steps = useMemo(() => path.slice(0, 32), [path]);
  const replaySteps = useMemo(() => steps.flatMap((step, index) => step.toNext && step.wire && index < steps.length - 1 ? [{ index, nodes: [], wires: [step.wire] }] : []), [steps]);
  const replay = useBlueprintReplay(replaySteps, run.key, 850);
  const active = replay && !replay.done ? replaySteps[Math.floor(replay.overall * replaySteps.length)]?.index : null;
  return <div aria-label="Run path" className="mt-2 max-h-64 overflow-auto">
    {run.events.some((event) => event.classifier) && <div className="mb-1 flex flex-wrap gap-1">{Array.from(new Map(run.events.filter((event) => event.classifier).map((event) => [event.classifier!.label, event.classifier!])).values()).map((kind) => <NodeKindBadge key={kind.label} kind={kind} />)}</div>}
    <ol className="flex min-w-max items-center gap-1 py-1">{steps.map((step, index) => <li key={index} className="flex items-center gap-1">
      <span className="max-w-40 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 py-1 text-xs">
        <span className="block truncate font-medium">{step.name}</span>
        <span className="block truncate text-[var(--ct-muted-fg)]">{step.outcome}{step.duration === null ? "" : ` · ${step.duration} ms`}</span>
        <span className="block text-[var(--ct-muted-fg)]">{new Date(step.at).toLocaleTimeString()}</span>
      </span>
      {step.toNext && index < steps.length - 1 && <svg width="24" height="20" viewBox="0 0 24 20" aria-hidden="true" className="shrink-0 text-[var(--ct-accent)]">
        <path d="M2 10 H21 M17 6 L21 10 L17 14" fill="none" stroke="currentColor" />
        {active === index && <circle cx={2 + 19 * (replay?.progress ?? 0)} cy="10" r="2" fill="currentColor" />}
      </svg>}
    </li>)}</ol>
    {path.length > steps.length && <p className="text-xs text-[var(--ct-muted-fg)]">{path.length - steps.length} more nodes in the full graph.</p>}
    {run.incomplete && <p className="text-xs text-[var(--ct-muted-fg)]">Some earlier events are outside retained history.</p>}
  </div>;
}
