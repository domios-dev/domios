import { AlertTriangle, CheckCircle2, Download, Loader2, RotateCw, X } from "lucide-react";
import { useUpdate, type UpdateStatus } from "../hooks/useUpdate";

const STATUS_COPY: Record<UpdateStatus, { title: string; body: string }> = {
  idle: { title: "Update", body: "Ready" },
  checking: { title: "Preparing update", body: "Checking the latest release." },
  "up-to-date": { title: "CodeTerm is up to date", body: "No update is needed." },
  available: { title: "Update available", body: "A new version is ready." },
  downloading: { title: "Downloading update", body: "Fetching the new CodeTerm build." },
  applying: { title: "Finishing update", body: "Please wait while CodeTerm prepares to restart." },
  restarting: { title: "Restarting CodeTerm", body: "The update is ready. Restart is in progress." },
  error: { title: "Update failed", body: "The update could not be completed." },
};

export function UpdateProgressLayer() {
  const update = useUpdate();
  if (!update.toastOpen && !update.blocking) return null;

  if (update.blocking) {
    return <UpdateBlockingOverlay />;
  }

  return <UpdateToast />;
}

function UpdateToast() {
  const {
    status,
    latestVersion,
    progress,
    downloadedBytes,
    downloadTotalBytes,
    dismissUpdateToast,
    error,
  } = useUpdate();
  const copy = STATUS_COPY[status];
  const isError = status === "error";
  const showProgress = status === "checking" || status === "downloading" || status === "applying";

  return (
    <div className="fixed right-3 top-11 z-[80] pointer-events-none">
      <div className="pointer-events-auto w-[min(360px,calc(100vw-1.5rem))] rounded-lg border border-[var(--ct-border)] bg-[var(--ct-sidebar-bg)] shadow-2xl ct-float">
        <div className="flex items-start gap-3 px-3 py-3">
          <div className={`mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md ${isError ? "bg-[var(--ct-red)]/15 text-[var(--ct-red)]" : "bg-[var(--ct-accent)]/15 text-[var(--ct-accent)]"}`}>
            {isError ? <AlertTriangle size={16} /> : <Download size={16} />}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center justify-between gap-2">
              <div className="truncate text-sm font-medium text-[var(--ct-fg)]">
                {latestVersion && !isError ? `${copy.title} v${latestVersion}` : copy.title}
              </div>
              <button
                type="button"
                onClick={dismissUpdateToast}
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
                title="Dismiss"
              >
                <X size={14} />
              </button>
            </div>
            <div className="mt-1 text-xs leading-relaxed text-[var(--ct-muted-fg)]">
              {error || copy.body}
            </div>
            {showProgress && (
              <ProgressBar
                progress={progress}
                downloadedBytes={downloadedBytes}
                downloadTotalBytes={downloadTotalBytes}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function UpdateBlockingOverlay() {
  const { status, latestVersion, progress, downloadedBytes, downloadTotalBytes } = useUpdate();
  const copy = STATUS_COPY[status];

  return (
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/55 px-4 backdrop-blur-sm">
      <div className="w-full max-w-[380px] rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)] p-5 shadow-2xl">
        <div className="mx-auto flex h-10 w-10 items-center justify-center rounded-lg bg-[var(--ct-accent)]/15 text-[var(--ct-accent)]">
          {status === "restarting" ? <RotateCw size={20} className="animate-spin" /> : <Loader2 size={20} className="animate-spin" />}
        </div>
        <div className="mt-4 text-center text-base font-semibold text-[var(--ct-fg)]">
          {latestVersion ? `${copy.title} v${latestVersion}` : copy.title}
        </div>
        <div className="mt-2 text-center text-sm leading-relaxed text-[var(--ct-muted-fg)]">
          {copy.body}
        </div>
        <ProgressBar
          progress={progress}
          downloadedBytes={downloadedBytes}
          downloadTotalBytes={downloadTotalBytes}
          centered
        />
      </div>
    </div>
  );
}

function ProgressBar({
  progress,
  downloadedBytes,
  downloadTotalBytes,
  centered,
}: {
  progress: number;
  downloadedBytes: number;
  downloadTotalBytes: number | null;
  centered?: boolean;
}) {
  const pct = Math.max(0, Math.min(100, Math.round(progress)));
  const detail =
    downloadTotalBytes && downloadTotalBytes > 0
      ? `${formatBytes(downloadedBytes)} / ${formatBytes(downloadTotalBytes)}`
      : null;

  return (
    <div className="mt-3">
      <div className="h-1.5 overflow-hidden rounded-full bg-[var(--ct-border)]">
        <div
          className="h-full rounded-full bg-[var(--ct-accent)] transition-[width] duration-300"
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className={`mt-1.5 flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)] ${centered ? "justify-center" : "justify-between"}`}>
        <span>{pct}%</span>
        {detail && <span>{detail}</span>}
        {pct >= 100 && <CheckCircle2 size={12} className="text-[var(--ct-green)]" />}
      </div>
    </div>
  );
}

function formatBytes(value: number): string {
  if (value <= 0) return "0 MB";
  const mb = value / 1024 / 1024;
  if (mb < 10) return `${mb.toFixed(1)} MB`;
  return `${Math.round(mb)} MB`;
}
