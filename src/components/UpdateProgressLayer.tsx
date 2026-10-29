import { X } from "lucide-react";
import { useUpdate } from "../hooks/useUpdate";
import { updatePresentation } from "../lib/updatePresentation";
import { RestoreStatePanel } from "./Terminal/RestoreStatePanel";
import type { RestoreStageView } from "../lib/restoreStageView";
import { formatUpdateBytes, updateStageView } from "../lib/updateStageView";
import { Button } from "./shared/Button";

export function UpdateProgressLayer() {
  const { status, latestVersion, error, toastOpen, installing, downloadedBytes, downloadTotalBytes,
    dismissUpdateToast, applyUpdate } = useUpdate();
  const presentation = updatePresentation(status, installing, latestVersion);
  if (presentation.blocking) {
    const stage = status === "restarting" ? "restarting" : "applying";
    return <UpdateScreen view={updateStageView(stage, latestVersion)} />;
  }
  if (!toastOpen || !presentation.background) return null;
  const measured = status === "downloading" && downloadTotalBytes && downloadTotalBytes > 0
    ? Math.min(100, Math.max(0, downloadedBytes / downloadTotalBytes * 100)) : undefined;
  return <aside aria-label="App update" className="fixed bottom-4 right-4 z-[90] w-80 max-w-[calc(100vw-2rem)] rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 shadow-lg">
    <div className="flex items-start justify-between gap-3">
      <div role="status" className="text-sm text-[var(--ct-fg)]">
        {presentation.title}{!presentation.retry && latestVersion ? ` ${latestVersion}` : ""}
      </div>
      <Button variant="ghost" size="compact" aria-label="Dismiss update progress" onClick={dismissUpdateToast}><X size={14} /></Button>
    </div>
    {presentation.retry ? <>
      <p className="mt-2 break-words text-xs text-[var(--ct-muted-fg)]">{error ?? "The update could not be completed."}</p>
      <Button variant="ghost" size="compact" onClick={() => void applyUpdate()}>Retry</Button>
    </> : <>
      <progress aria-label="Domios update progress" max={100} value={measured} className="ct-progress mt-2" />
      <p className="mt-2 text-xs text-[var(--ct-muted-fg)]">
        {measured !== undefined && downloadTotalBytes !== null ? `${formatUpdateBytes(downloadedBytes)} of ${formatUpdateBytes(downloadTotalBytes)}` : "You can keep working while the update is prepared."}
      </p>
    </>}
  </aside>;
}

export function UpdateScreen({ view, contained = false }: { view: RestoreStageView; contained?: boolean }) {
  return (
    <div data-testid="update-screen" className={`${contained ? "absolute" : "fixed"} inset-0 z-[90] bg-[var(--ct-bg)]`}>
      <RestoreStatePanel view={view} pulse />
    </div>
  );
}
