import { createNoticePublisher } from "../../lib/notices";
import { useCallback, useEffect, useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { ArrowDownToLine, CheckCircle2, CircleAlert, LoaderCircle } from "lucide-react";
import { useUpdatePanel } from "../../hooks/useUpdatePanel";
import { useUpdate, type UpdatePhase } from "../../hooks/useUpdate";
import { useMultiplexer } from "../../store/multiplexer/context";
import { describeError } from "../../lib/describeError";
import { UpdatePanel } from "../UpdatePanel";
import { updateIndicator } from "../../lib/updateStatusModel";
import { unavailableUpdateSources } from "../../lib/updatePanelModel";

function knownDownloadPercent(completed: number, total: number | null): number | null {
  if (total === null || !Number.isFinite(total) || total <= 0
    || !Number.isFinite(completed) || completed < 0 || completed > total) return null;
  return Math.round(completed / total * 100);
}

function appPhaseDescription(phase: UpdatePhase): string | null {
  switch (phase) {
    case "checking": return "Checking for app updates.";
    case "downloading": return "Downloading app update.";
    case "applying": return "Applying app update.";
    default: return null;
  }
}

function DownloadProgress({ value }: { value: number }) {
  const radius = 6;
  const circumference = 2 * Math.PI * radius;
  return <span role="progressbar" aria-label="App download progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={value}>
    <svg aria-hidden="true" viewBox="0 0 16 16" width={16} height={16}>
      <circle cx={8} cy={8} r={radius} fill="none" stroke="currentColor" strokeOpacity={0.25} strokeWidth={2} />
      <circle cx={8} cy={8} r={radius} fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round"
        strokeDasharray={circumference} strokeDashoffset={circumference * (1 - value / 100)} transform="rotate(-90 8 8)" />
    </svg>
  </span>;
}

export function UpdateTray() {
  const update = useUpdatePanel();
  const app = useUpdate();
  const { upsertToast } = useMultiplexer();
  const [open, setOpen] = useState(false);
  const toastedError = useRef<string | null>(null);
  const toastedFrontend = useRef<string | null>(null);
  useEffect(() => {
    const release = update.frontendRelease;
    if (!release || toastedFrontend.current === release.releaseId) return;
    toastedFrontend.current = release.releaseId;
    createNoticePublisher({ upsertToast })({ kind: "toast", id: "frontend-update-completed",
      title: `Interface ${release.version} updated`, detail: release.notes ? `What’s new: ${release.notes}` : "No restart required.",
      severity: "Info", tabId: null, workspaceId: null });
  }, [update.frontendRelease, upsertToast]);
  useEffect(() => {
    if (!update.error) { toastedError.current = null; return; }
    if (toastedError.current === update.error) return;
    toastedError.current = update.error;
    const description = describeError(update.error, { operation: "updating components" });
    createNoticePublisher({ upsertToast })({ kind: "toast", id: "component-update-failure",
      title: description.title,
      detail: description.cause,
      severity: "Warning",
      tabId: null,
      workspaceId: null,
      actions: [
        ...(description.raw ? [{ label: "Copy details", run: async () => {
              try {
                await navigator.clipboard.writeText(description.raw);
              }
              catch {
                return;
              }
            } }] : []),
        { label: "Retry", run: () => update.retry() },
      ] });
  }, [update.error, update.retry, upsertToast]);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useEffect(() => {
    if (open) panel.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [open]);
  const indicator = updateIndicator(update.rows, { busy: update.busy, checking: update.checking,
    failed: Boolean(update.error), completed: Boolean(update.frontendRelease), unavailableSources: unavailableUpdateSources(update.rows, update.diagnostics) });
  const { offers, active, state: activityState } = indicator;
  const failed = activityState === "failed";
  const appPhase = app.installing ? appPhaseDescription(app.phase) : null;
  const downloadPercent = !failed && app.installing && app.phase === "downloading"
    ? knownDownloadPercent(app.downloadedBytes, app.downloadTotalBytes) : null;
  const activityDescription = !failed && !update.checking && appPhase
    ? `${appPhase}${downloadPercent === null ? "" : ` ${downloadPercent}% downloaded.`}` : indicator.description;
  const triggerTone = failed ? "bg-[var(--ct-surface)] text-[var(--ct-red)]"
    : active || offers ? "bg-[var(--ct-accent)] text-[var(--ct-on-accent)]"
      : activityState === "unavailable" ? "bg-[var(--ct-surface)] text-[var(--ct-yellow)]"
        : activityState === "completed" ? "bg-[var(--ct-surface)] text-[var(--ct-green)]"
          : "bg-[var(--ct-surface)] text-[var(--ct-muted-fg)]";
  return <>
    <button ref={trigger} type="button" data-testid="settings-update-action" aria-label="Update" aria-description={activityDescription}
      aria-haspopup="dialog" aria-expanded={open} aria-busy={active} data-activity-state={activityState}
      onClick={() => setOpen(!open)} title={offers ? "Updates available" : "Update history"}
      className={`inline-flex h-6 w-6 items-center justify-center rounded-full ${triggerTone} hover:brightness-110`}>
      {downloadPercent !== null ? <DownloadProgress value={downloadPercent} />
        : failed || activityState === "unavailable" ? <CircleAlert size={15} aria-hidden="true" />
          : active ? <LoaderCircle size={15} aria-hidden="true" className="motion-safe:animate-spin" />
            : activityState === "completed" ? <CheckCircle2 size={15} aria-hidden="true" />
              : <ArrowDownToLine size={15} aria-hidden="true" />}
    </button>
    <AnchoredPopover open={open} anchor={trigger} placement="bottom-end" onClose={close}>
      <div id="update-tray-dialog" ref={panel} role="dialog" aria-label="Updates" className="w-[400px] max-w-[calc(100vw-24px)] max-h-[min(640px,80vh)] overflow-y-auto">
        <UpdatePanel />
      </div>
    </AnchoredPopover>
  </>;
}
