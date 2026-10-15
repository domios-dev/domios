import { useState, useEffect } from "react";
import { InlineErrorNotice } from "./shared/ErrorNotice";
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
      data-testid="stale-build-banner"
      className="fixed top-0 inset-x-0 z-[9999] flex items-start gap-2 px-4 py-2.5 shadow-lg"
      style={{ background: "var(--ct-surface)", borderBottom: "1px solid var(--ct-border)" }}
    >
      <InlineErrorNotice error={{ code: "frontend_version_mismatch", raw: `Interface API range: ${mismatch.minApiVersion}–${mismatch.maxApiVersion}; daemon API: ${mismatch.apiVersion ?? "unknown"}` }}
        actions={[{ label: "Reload interface", onClick: () => window.location.reload() }, { label: "Dismiss", onClick: () => setDismissed(true) }]} />
    </div>
  );
}
