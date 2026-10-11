import { useState, useEffect } from "react";
import { AlertTriangle, X } from "lucide-react";
import { checkBuildMismatch, onMismatch, type BuildMismatch } from "../lib/buildMismatchGuard";
export function StaleBuildBanner() {
  const [mismatch, setMismatch] = useState<BuildMismatch | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    const unlisten = onMismatch(setMismatch);
    void checkBuildMismatch().catch(() => {});

    return unlisten;
  }, []);

  if (!mismatch || dismissed) return null;

  return (
    <div
      role="alert"
      data-testid="stale-build-banner"
      className="fixed top-0 inset-x-0 z-[9999] flex items-start gap-2 px-4 py-2.5 shadow-lg"
      style={{
        background: "color-mix(in srgb, var(--ct-yellow) 15%, var(--ct-surface))",
        borderBottom: "1px solid color-mix(in srgb, var(--ct-yellow) 40%, transparent)",
      }}
    >
      <AlertTriangle
        size={14}
        className="shrink-0 mt-0.5"
        style={{ color: "var(--ct-yellow)" }}
      />
      <span className="flex-1 text-[11px]" style={{ color: "var(--ct-fg)" }}>
        <span className="font-semibold">Frontend update required</span>
        {" — "}This frontend supports API {mismatch.minApiVersion}–{mismatch.maxApiVersion};
        the daemon reports {mismatch.apiVersion ?? "an unknown API version"}.

      </span>
      <button
        onClick={() => setDismissed(true)}
        aria-label="Dismiss"
        className="shrink-0 text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
      >
        <X size={12} strokeWidth={1.5} />
      </button>
    </div>
  );
}
