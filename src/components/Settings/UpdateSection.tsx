import { useUpdate } from "../../hooks/useUpdate";
import type { UpdateChannel } from "../../hooks/useUpdate";
import { Group } from "./shared";

const CHANNELS: { value: UpdateChannel; label: string; hint: string }[] = [
  { value: "stable", label: "Stable", hint: "Production releases" },
  { value: "rc", label: "RC", hint: "Release candidates" },
  { value: "nightly", label: "Nightly", hint: "Bleeding edge" },
];

export function UpdateSection() {
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
  } = useUpdate();

  const updateAvailable = status === "available" && latestVersion && latestVersion !== currentVersion;

  return (
    <Group title="Updates">
      <div className="space-y-3">
        <div className="flex items-center gap-3 flex-wrap">
          <button
            onClick={() => void refresh(true)}
            disabled={applying || status === "checking"}
            className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-muted)] transition-colors disabled:opacity-50"
          >
            {status === "checking" ? "Checking..." : "Check for Updates"}
          </button>
          {status === "up-to-date" && (
            <span className="text-[11px] text-[var(--ct-green)]">Up to date</span>
          )}
          {updateAvailable && (
            <>
              <span className="text-[11px] text-[var(--ct-accent)]">v{latestVersion} available</span>
              <button
                onClick={() => void applyUpdate()}
                disabled={applying}
                className="px-3 py-1.5 text-[11px] rounded bg-[var(--ct-accent)] text-white hover:opacity-90 transition-opacity font-medium disabled:opacity-50"
              >
                {applying ? "Installing..." : "Update now"}
              </button>
            </>
          )}
          {error && (
            <span className="text-[11px] text-[var(--ct-red)]">{error}</span>
          )}
        </div>

        {applying && (
          <div className="p-2 rounded bg-[var(--ct-accent)]/10 border border-[var(--ct-accent)]/30 text-[var(--ct-accent)] text-[11px]">
            Downloading and installing... The app will restart automatically.
          </div>
        )}

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
                    onClick={() => void setChannel(c.value)}
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

        {updateAvailable && releaseNotes && releaseNotes.trim().length > 0 && (
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
              style={{ maxHeight: 200, overflowY: "auto" }}
            >
              {releaseNotes}
            </div>
          </div>
        )}
      </div>
    </Group>
  );
}
