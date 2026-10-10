import { useRef, useState } from "react";
import { AlertTriangle, FileText, Map as MapIcon, RefreshCw } from "lucide-react";
import { SendIcon } from "../Icons";
import { UI_BUTTON } from "../shared/design";
import { projectName, statusLabel } from "./EssenceMapProjectView";
import type { EssenceMapProject } from "./essenceMapModel";
import type { MapOrchestrator } from "./essenceMapOrchestrator";

const ATTENTION_STATUSES = new Set(["malformed", "oversized", "unreadable"]);

export interface EssenceMapEmptyStateProps {
  projects: EssenceMapProject[];
  orchestrator: MapOrchestrator | null;
  onAskOrchestrator?: (orchestrator: MapOrchestrator) => Promise<void>;
  onRetry?: () => void;
  onCreateMap?: () => void;
}

export function EssenceMapEmptyState({ projects, orchestrator, onAskOrchestrator, onRetry, onCreateMap }: EssenceMapEmptyStateProps) {
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const canAsk = orchestrator !== null && onAskOrchestrator !== undefined;
  const ask = async () => {
    if (!orchestrator || !onAskOrchestrator || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    setError(null);
    try {
      await onAskOrchestrator(orchestrator);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      inFlight.current = false;
      setSending(false);
    }
  };
  return <div data-essence-map-control className="absolute inset-0 z-10 flex cursor-default items-center justify-center overflow-auto px-6 pb-16 pt-6">
    <section data-testid="essence-map-no-nodes" aria-labelledby="essence-map-empty-title" aria-describedby="essence-map-empty-next" aria-busy={sending} className="flex w-full max-w-md flex-col items-center gap-3 text-center">
      <MapIcon size={22} aria-hidden="true" className="text-[var(--ct-muted-fg)]" />
      <h2 id="essence-map-empty-title" className="text-sm font-semibold text-[var(--ct-fg)]">No project map yet</h2>
      <ul aria-label="Project map status" className="flex w-full flex-col gap-1 text-xs text-[var(--ct-muted-fg)]">
        {projects.map((project) => <li key={project.file.project_path} data-testid={`essence-map-project-${project.status}`} className="flex min-w-0 items-center justify-center gap-2" title={project.file.project_path}>
          {ATTENTION_STATUSES.has(project.status) ? <AlertTriangle size={13} aria-hidden="true" className="shrink-0 text-[var(--ct-yellow)]" /> : <FileText size={13} aria-hidden="true" className="shrink-0" />}
          <span className="truncate"><span className="font-medium text-[var(--ct-fg)]">{projectName(project.file.project_path)}</span> · {project.error ?? statusLabel(project)}</span>
        </li>)}
      </ul>
      <p id="essence-map-empty-next" className="text-xs text-[var(--ct-muted-fg)]">
        {orchestrator ? `Ask the Orchestrator (${orchestrator.title}) to build the project map. It knows how.` : "Start an Orchestrator in this workspace, then ask it to build the project map. It knows how."}
      </p>
      <div className="flex flex-wrap items-center justify-center gap-2">
        {onCreateMap && <button type="button" className={UI_BUTTON.primary} onClick={onCreateMap}>Create map</button>}
        {canAsk && <button type="button" data-testid="essence-map-ask-orchestrator" className={`${UI_BUTTON.ghost} inline-flex items-center gap-1.5`} disabled={sending} onClick={() => void ask()}>
          <SendIcon size={13} />{sending ? "Sending…" : "Ask the Orchestrator to build the project map"}
        </button>}
        {onRetry && <button type="button" className={`${UI_BUTTON.ghost} inline-flex items-center gap-1.5`} onClick={onRetry}><RefreshCw size={12} aria-hidden="true" />Check again</button>}
      </div>
      {error && <p role="alert" className="text-xs text-[var(--ct-red)]">{error}</p>}
    </section>
  </div>;
}
