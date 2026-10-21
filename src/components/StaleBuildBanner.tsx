import { useCallback, useEffect } from "react";
import { reloadInterface } from "../lib/serviceWorkerRegistration";
import { checkBuildMismatch, installBuildDriftTriggers, onMismatch, type BuildMismatch } from "../lib/buildMismatchGuard";
import { describeError } from "../lib/describeError";
import { useMultiplexer } from "../store/multiplexer";

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
  const { dismissToast, upsertToast } = useMultiplexer();
  const reload = useCallback(async (mismatch: BuildMismatch) => {
    const attempt = async () => {
      try {
        await reloadInterface(mismatch.backendHash);
      } catch (error) {
        const description = describeError(error, { operation: "reloading the interface" });
        upsertToast({
          id: "interface-reload-failure",
          title: description.title,
          body: description.cause,
          severity: "Warning",
          level: "action_needed",
          tab_id: null,
          workspace_id: null,
          local_actions: [
            { label: "Copy details", run: async () => {
              try { await navigator.clipboard.writeText(description.raw); } catch { return; }
            } },
            { label: "Retry", run: attempt },
          ],
        });
      }
    };
    await attempt();
  }, [upsertToast]);
  const notifyMismatch = useCallback((mismatch: BuildMismatch) => {
    const description = describeError(noticeFor(mismatch));
    upsertToast({
      id: "interface-build-mismatch",
      title: description.title,
      body: description.cause,
      severity: "Info",
      level: "action_needed",
      tab_id: null,
      workspace_id: null,
      local_actions: [
        { label: "Reload interface", run: () => reload(mismatch) },
        { label: "Copy details", run: async () => {
          try { await navigator.clipboard.writeText(description.raw); } catch { return; }
        } },
      ],
    });
  }, [reload, upsertToast]);

  useEffect(() => {
    const unlisten = onMismatch((next) => {
      if (next) notifyMismatch(next);
      else dismissToast("interface-build-mismatch");
    });
    const uninstall = installBuildDriftTriggers();
    void checkBuildMismatch().catch(() => {});

    return () => { unlisten(); uninstall(); };
  }, [dismissToast, notifyMismatch]);
  return null;
}
