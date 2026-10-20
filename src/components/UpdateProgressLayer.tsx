import { useEffect } from "react";
import { useMultiplexer } from "../store/multiplexer";
import { useUpdate, type UpdateStatus } from "../hooks/useUpdate";
import { updateNotice } from "../lib/updateNotice";
import { RestoreStatePanel } from "./Terminal/RestoreStatePanel";
import type { RestoreStageView } from "../lib/restoreStageView";
import { updateStageView } from "../lib/updateStageView";

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
  const { status, latestVersion, error, toastOpen, blocking, dismissUpdateToast, applyUpdate } = update;
  useEffect(() => {
    if (!toastOpen || blocking || status !== "error") return;
    const copy = STATUS_COPY[status];
    const notice = updateNotice({ ...copy, latestVersion, error, failed: status === "error" });
    upsertToast({ id: "app-update", tab_id: null, workspace_id: null, ...notice,
      local_actions: notice.retry ? [{ label: "Retry", run: () => { void applyUpdate(); } }] : undefined,
    });
    dismissUpdateToast();
  }, [status, latestVersion, error, toastOpen, blocking, dismissUpdateToast, upsertToast, applyUpdate]);
  return blocking ? <UpdateBlockingOverlay /> : null;
}

function UpdateBlockingOverlay() {
  const { status, latestVersion, downloadedBytes, downloadTotalBytes } = useUpdate();
  const stage = status === "restarting" ? "restarting" : status === "applying" ? "applying" : "downloading";
  return <UpdateScreen view={updateStageView(stage, latestVersion, downloadedBytes, downloadTotalBytes)} />;
}

/** Full-screen update stage; `contained` keeps it inside its parent for previews. */
export function UpdateScreen({ view, contained = false }: { view: RestoreStageView; contained?: boolean }) {
  return (
    <div data-testid="update-screen" className={`${contained ? "absolute" : "fixed"} inset-0 z-[90] bg-[var(--ct-bg)]`}>
      <RestoreStatePanel view={view} />
    </div>
  );
}
