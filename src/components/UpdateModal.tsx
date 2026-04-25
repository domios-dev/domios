import { useEffect, useRef } from "react";
import type { UpdateChannel } from "../hooks/useUpdate";
import { useUpdate } from "../hooks/useUpdate";

interface UpdateModalProps {
  update: ReturnType<typeof useUpdate>;
  onClose: () => void;
}

const CHANNELS: { value: UpdateChannel; label: string; hint: string }[] = [
  { value: "stable", label: "Stable", hint: "Production releases" },
  { value: "rc", label: "RC", hint: "Release candidates" },
  { value: "nightly", label: "Nightly", hint: "Bleeding edge" },
];

export function UpdateModal({ update, onClose }: UpdateModalProps) {
  const {
    status,
    currentVersion,
    latestVersion,
    releaseNotes,
    releaseUrl,
    channel,
    channelSupported,
    setChannel,
    refresh,
    applyUpdate,
    applying,
    error,
  } = update;

  const backdropRef = useRef<HTMLDivElement>(null);
  const onBackdropClick = (e: React.MouseEvent) => {
    if (e.target === backdropRef.current && !applying) onClose();
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !applying) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [applying, onClose]);

  const updateAvailable = status === "available" && latestVersion && latestVersion !== currentVersion;

  return (
    <div
      ref={backdropRef}
      onClick={onBackdropClick}
      className="fixed inset-0 z-[100] flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.45)" }}
    >
      <div
        className="flex flex-col bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg shadow-2xl"
        style={{ width: "min(560px, 90vw)", maxHeight: "min(640px, 85vh)" }}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--ct-border)]">
          <div className="flex items-center gap-2">
            <span className="text-[12px] font-medium text-[var(--ct-fg)]">
              {updateAvailable ? "Update available" : "CodeTerm updates"}
            </span>
            {currentVersion && (
              <span className="text-[10px] text-[var(--ct-muted-fg)]">
                v{currentVersion}
                {updateAvailable && (
                  <>
                    <span className="mx-1.5 opacity-60">→</span>
                    <span className="text-[var(--ct-accent)]">v{latestVersion}</span>
                  </>
                )}
              </span>
            )}
          </div>
          <button
            onClick={onClose}
            disabled={applying}
            className="text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] text-sm px-1 disabled:opacity-40"
          >
            ✕
          </button>
        </div>

        <div className="flex-1 overflow-y-auto px-4 py-3 space-y-4">
          {channelSupported && (
            <div>
              <label className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)] block mb-1.5">
                Release channel
              </label>
              <div className="flex gap-1.5">
                {CHANNELS.map((c) => {
                  const active = channel === c.value;
                  return (
                    <button
                      key={c.value}
                      onClick={() => setChannel(c.value)}
                      disabled={applying || channel === null}
                      title={c.hint}
                      className={
                        "flex-1 px-2.5 py-1.5 rounded border text-[11px] transition-colors disabled:opacity-50 " +
                        (active
                          ? "bg-[var(--ct-accent)] text-white border-[var(--ct-accent)]"
                          : "bg-transparent text-[var(--ct-fg)] border-[var(--ct-border)] hover:bg-[var(--ct-hover)]")
                      }
                    >
                      {c.label}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          <div>
            <div className="flex items-center justify-between mb-1.5">
              <label className="text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">
                Release notes
              </label>
              {releaseUrl && (
                <a
                  href={releaseUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="text-[10px] text-[var(--ct-accent)] hover:underline"
                >
                  Open on GitHub
                </a>
              )}
            </div>
            <div
              className="bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded p-3 text-[11px] text-[var(--ct-fg)] font-mono leading-relaxed whitespace-pre-wrap"
              style={{ maxHeight: 280, overflowY: "auto" }}
            >
              {releaseNotes && releaseNotes.trim().length > 0
                ? releaseNotes
                : updateAvailable
                  ? "No release notes provided."
                  : "You are running the latest version."}
            </div>
          </div>

          {error && (
            <div className="p-2 rounded bg-[var(--ct-red)]/15 border border-[var(--ct-red)]/30 text-[var(--ct-red)] text-[11px]">
              {error}
            </div>
          )}

          {applying && (
            <div className="p-2 rounded bg-[var(--ct-accent)]/10 border border-[var(--ct-accent)]/30 text-[var(--ct-accent)] text-[11px]">
              Downloading and installing… The app will restart automatically.
            </div>
          )}
        </div>

        <div className="flex items-center justify-between gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
          <button
            onClick={() => refresh(true)}
            disabled={applying || status === "checking"}
            className="px-2.5 py-1.5 text-[11px] rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-40"
          >
            {status === "checking" ? "Checking…" : "Recheck"}
          </button>
          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              disabled={applying}
              className="px-3 py-1.5 text-[11px] rounded border border-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-40"
            >
              Cancel
            </button>
            <button
              onClick={() => void applyUpdate()}
              disabled={applying || !updateAvailable}
              className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-accent)] text-white font-medium hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {applying ? "Installing…" : "Update now"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
