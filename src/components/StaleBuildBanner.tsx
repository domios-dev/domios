import { useState, useEffect } from "react";
import { AlertTriangle, X } from "lucide-react";
import { checkBuildMismatch, onMismatch, type BuildMismatch } from "../lib/buildMismatchGuard";
import { IS_WEB } from "../lib/webApi";

// No-op during Tauri E2E tests or wherever __BUILD_HASH__ is "dev".
// The check is gated inside checkBuildMismatch itself (skips "dev" hash).

export function StaleBuildBanner() {
  const [mismatch, setMismatch] = useState<BuildMismatch | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    // Subscribe before checking so we don't miss a synchronous resolution.
    const unlisten = onMismatch(setMismatch);

    // Derive API base: in Tauri use the local dev daemon; in web use current origin.
    const apiBase = IS_WEB
      ? window.location.origin
      : "http://127.0.0.1:7686";

    checkBuildMismatch(apiBase).catch(() => {});

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
        <span className="font-semibold">Runtime identity mismatch</span>
        {" — "}
        daemon <code className="font-mono">{mismatch.backendVersion}</code>/
        <code className="font-mono">{mismatch.backendHash}</code>, UI expects{" "}
        <code className="font-mono">{mismatch.frontendVersion}</code>/
        <code className="font-mono">{mismatch.frontendHash}</code>. Restart dev:{" "}
        <code className="font-mono">./build.sh dev</code>
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
