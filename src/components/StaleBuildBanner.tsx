import { useState, useEffect } from "react";
import { InlineErrorNotice } from "./shared/ErrorNotice";
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

  useEffect(() => {
    const unlisten = onMismatch((next) => { setMismatch(next); setDismissed(false); });
    const uninstall = installBuildDriftTriggers();
    void checkBuildMismatch().catch(() => {});

    return () => { unlisten(); uninstall(); };
  }, []);

  if (!mismatch || dismissed) return null;

  return (
    <div
      data-testid="stale-build-banner"
      className="fixed top-0 inset-x-0 z-[9999] flex items-start gap-2 px-4 py-2.5 shadow-lg"
      style={{ background: "var(--ct-surface)", borderBottom: "1px solid var(--ct-border)" }}
    >
      <InlineErrorNotice error={noticeFor(mismatch)}
        actions={[{ label: "Reload interface", onClick: () => window.location.reload() }, { label: "Dismiss", onClick: () => setDismissed(true) }]} />
    </div>
  );
}
