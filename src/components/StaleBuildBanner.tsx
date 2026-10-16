import { useState, useEffect } from "react";
import { Info, RefreshCw, X } from "lucide-react";
import { reloadInterface } from "../lib/serviceWorkerRegistration";
import { checkBuildMismatch, installBuildDriftTriggers, onMismatch, type BuildMismatch } from "../lib/buildMismatchGuard";

function noticeFor(mismatch: BuildMismatch): { code: string; raw: string } {
  if (mismatch.reason === "api") {
    return { code: "frontend_version_mismatch", raw: `Interface API range: ${mismatch.minApiVersion}–${mismatch.maxApiVersion}; daemon API: ${mismatch.apiVersion ?? "unknown"}` };
  }
  return {
    code: mismatch.reason === "build" ? "frontend_build_drift" : "frontend_build_drift_unreachable",
    raw: `Interface build: ${mismatch.frontendHash}; daemon build: ${mismatch.backendHash}`,
  };
}

export function StaleBuildBanner() {
  const [mismatch, setMismatch] = useState<BuildMismatch | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [details, setDetails] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = onMismatch((next) => { setMismatch(next); setDismissed(false); });
    const uninstall = installBuildDriftTriggers();
    void checkBuildMismatch().catch(() => {});

    return () => { unlisten(); uninstall(); };
  }, []);

  if (!mismatch || dismissed) return null;
  const reload = async () => {
    setUpdating(true); setFailure(null);
    try { await reloadInterface(mismatch.backendHash); }
    catch (error) { setFailure(String(error)); }
    finally { setUpdating(false); }
  };

  return (
    <div
      data-testid="stale-build-banner"
      role="status"
      aria-label="Interface update"
      className="fixed right-3 top-[max(0.75rem,env(safe-area-inset-top))] z-[9999] max-w-[calc(100vw-1.5rem)] rounded-2xl border border-[var(--ct-border)] bg-[var(--ct-surface)] p-1 text-xs text-[var(--ct-fg)] shadow-lg"
    >
      <div className="flex items-center gap-1">
        <button type="button" aria-label="Reload interface" disabled={updating} onClick={() => { void reload(); }}
          className="flex items-center gap-1.5 rounded-xl px-2 py-1.5 hover:bg-[var(--ct-bg-hover)] disabled:opacity-60">
          <RefreshCw size={13} aria-hidden="true" />
          {updating ? "Updating…" : mismatch.reason === "api" ? "Interface incompatible · Update" : "Interface update"}
        </button>
        <button type="button" aria-label="Interface update details" aria-expanded={details} onClick={() => setDetails(!details)} className="rounded-full p-1.5 hover:bg-[var(--ct-bg-hover)]"><Info size={13} aria-hidden="true" /></button>
        <button type="button" aria-label="Dismiss interface update" onClick={() => setDismissed(true)} className="rounded-full p-1.5 hover:bg-[var(--ct-bg-hover)]"><X size={13} aria-hidden="true" /></button>
      </div>
      {failure && <p role="alert" className="max-w-64 px-2 py-1">Update did not complete. Try again.</p>}
      {details && <div role="region" aria-label="Interface build details" className="max-w-72 break-words border-t border-[var(--ct-border)] px-2 py-2">
        {noticeFor(mismatch).raw}{failure && <p>{failure}</p>}
      </div>}
    </div>
  );
}
