import { useEffect } from "react";
import { AlertCircle, CheckCircle2, Download, ExternalLink, RefreshCw } from "lucide-react";
import { useUpdate } from "../../hooks/useUpdate";
import { UI_BUTTON } from "../shared/design";
import { SettingsCard, SettingsPill } from "./shared";

export function UpdateSection() {
  const {
    status,
    currentVersion,
    latestVersion,
    releaseNotes,
    releaseUrl,
    refresh,
    applyUpdate,
    installing,
    progress,
    downloadedBytes,
    downloadTotalBytes,
    error,
  } = useUpdate();

  useEffect(() => {
    void refresh(true);
  }, [refresh]);

  const checking = status === "checking" && !installing;
  const updateAvailable = status === "available" && latestVersion && latestVersion !== currentVersion;
  const statusTone: "red" | "accent" | "green" | "muted" = error
    ? "red"
    : updateAvailable
      ? "accent"
      : status === "up-to-date"
        ? "green"
        : "muted";
  const statusText = error
    ? "failed"
    : installing
      ? "updating"
      : checking
        ? "checking"
        : updateAvailable
          ? "available"
          : status === "up-to-date"
            ? "up to date"
            : "idle";

  return (
    <SettingsCard
      icon={<Download size={16} />}
      title="Updates"
      headerRight={<SettingsPill tone={statusTone}>{statusText}</SettingsPill>}
    >
      <div className="flex flex-col gap-3">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <div className="text-sm font-medium text-[var(--ct-fg)]">
              {updateAvailable ? `Version ${latestVersion} is ready` : "CodeTerm updates"}
            </div>
            <div className="mt-1 text-xs text-[var(--ct-muted-fg)]">
              Current version: {currentVersion ?? __APP_VERSION__}
            </div>
          </div>

          <div className="flex shrink-0 flex-wrap gap-2">
            {updateAvailable && (
              <button
                type="button"
                onClick={() => void applyUpdate()}
                disabled={installing}
                className={`${UI_BUTTON.primary} ${UI_BUTTON.sm} gap-1.5 disabled:opacity-50`}
              >
                <Download size={13} />
                <span>{installing ? "Updating..." : "Update now"}</span>
              </button>
            )}
            <button
              type="button"
              onClick={() => void refresh(true)}
              disabled={installing || checking}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} gap-1.5 disabled:opacity-50`}
            >
              <RefreshCw size={13} className={checking ? "animate-spin" : ""} />
              <span>{checking ? "Checking..." : "Check again"}</span>
            </button>
          </div>
        </div>

        {installing && (
          <div className="rounded-md border border-[var(--ct-accent)]/30 bg-[var(--ct-accent)]/10 px-3 py-2">
            <div className="flex items-center justify-between gap-2 text-xs text-[var(--ct-accent)]">
              <span>{installLabel(status)}</span>
              <span>{Math.max(1, Math.round(progress))}%</span>
            </div>
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[var(--ct-border)]">
              <div
                className="h-full rounded-full bg-[var(--ct-accent)] transition-[width] duration-300"
                style={{ width: `${Math.max(1, Math.min(100, Math.round(progress)))}%` }}
              />
            </div>
            {downloadTotalBytes && downloadTotalBytes > 0 && (
              <div className="mt-1.5 text-[10px] text-[var(--ct-muted-fg)]">
                {formatBytes(downloadedBytes)} / {formatBytes(downloadTotalBytes)}
              </div>
            )}
          </div>
        )}

        {status === "up-to-date" && !installing && (
          <div className="flex items-center gap-2 text-xs text-[var(--ct-green)]">
            <CheckCircle2 size={14} />
            <span>You are running the latest version.</span>
          </div>
        )}

        {error && (
          <div className="flex items-start gap-2 rounded-md border border-[var(--ct-red)]/30 bg-[var(--ct-red)]/10 px-3 py-2 text-xs text-[var(--ct-red)]">
            <AlertCircle size={14} className="mt-0.5 shrink-0" />
            <span>{error}</span>
          </div>
        )}

        {updateAvailable && releaseNotes && releaseNotes.trim().length > 0 && (
          <div>
            <div className="mb-1.5 flex items-center justify-between gap-2">
              <div className="text-[10px] uppercase text-[var(--ct-muted)]">Release notes</div>
              {releaseUrl && (
                <a
                  href={releaseUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1 text-[10px] text-[var(--ct-accent)] hover:underline"
                >
                  <ExternalLink size={11} />
                  <span>GitHub</span>
                </a>
              )}
            </div>
            <div className="max-h-[200px] overflow-y-auto rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] p-3 font-mono text-[11px] leading-relaxed text-[var(--ct-fg)] whitespace-pre-wrap">
              {releaseNotes}
            </div>
          </div>
        )}
      </div>
    </SettingsCard>
  );
}

function installLabel(status: string): string {
  switch (status) {
    case "checking":
      return "Preparing update";
    case "downloading":
      return "Downloading update";
    case "applying":
      return "Finishing update";
    case "restarting":
      return "Restarting CodeTerm";
    default:
      return "Updating CodeTerm";
  }
}

function formatBytes(value: number): string {
  if (value <= 0) return "0 MB";
  const mb = value / 1024 / 1024;
  if (mb < 10) return `${mb.toFixed(1)} MB`;
  return `${Math.round(mb)} MB`;
}
