import { useEffect } from "react";
import { useMultiplexer } from "../store/multiplexer";
import { CheckCircle2, Loader2, RotateCw } from "lucide-react";
import { useUpdate, type UpdateStatus } from "../hooks/useUpdate";
import { BusySpinner } from "./shared/BusyState";

const STATUS_COPY: Record<UpdateStatus, { title: string; body: string }> = {
  idle: { title: "Update", body: "Ready" },
  checking: { title: "Preparing update", body: "Checking the latest release." },
  "up-to-date": { title: "Domios is up to date", body: "No update is needed." },
  available: { title: "Update available", body: "A new version is ready." },
  downloading: { title: "Downloading update", body: "Fetching the new Domios build." },
  applying: { title: "Finishing update", body: "Please wait while Domios prepares to restart." },
  restarting: { title: "Restarting Domios", body: "The update is ready. Restart is in progress." },
  error: { title: "Update failed", body: "The update could not be completed." },
};

export function UpdateProgressLayer() {
  const update = useUpdate();
  const { upsertToast } = useMultiplexer();
  const { status, latestVersion, error, toastOpen, blocking, dismissUpdateToast } = update;
  useEffect(() => {
    if (!toastOpen || blocking) return;
    const copy = STATUS_COPY[status];
    upsertToast({ id: "app-update", tab_id: null, workspace_id: null, title: latestVersion ? `${copy.title} v${latestVersion}` : copy.title, body: error || copy.body, severity: status === "error" ? "Error" : "Info", level: status === "error" ? "failure" : "verbose" });
    dismissUpdateToast();
  }, [status, latestVersion, error, toastOpen, blocking, dismissUpdateToast, upsertToast]);
  return blocking ? <UpdateBlockingOverlay /> : null;
}

function UpdateBlockingOverlay() {
  const { status, latestVersion, progress, downloadedBytes, downloadTotalBytes } = useUpdate();
  const copy = STATUS_COPY[status];

  return (
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/55 px-4 backdrop-blur-sm">
      <div className="w-full max-w-[380px] rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)] p-5 shadow-2xl">
        <div className="mx-auto flex h-10 w-10 items-center justify-center rounded-lg bg-[var(--ct-accent)]/15 text-[var(--ct-accent)]">
          {status === "restarting" ? <BusySpinner icon={RotateCw} size={20} /> : <BusySpinner icon={Loader2} size={20} />}
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
