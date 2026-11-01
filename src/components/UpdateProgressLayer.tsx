import { useEffect } from "react";
import { useUpdate } from "../hooks/useUpdate";
import { useMultiplexer } from "../store/multiplexer";
import { createNoticePublisher } from "../lib/notices";
import { updateNotice } from "../lib/updateNotice";
import { updatePresentation } from "../lib/updatePresentation";
import { RestoreStatePanel } from "./Terminal/RestoreStatePanel";
import type { RestoreStageView } from "../lib/restoreStageView";
import { updateStageView } from "../lib/updateStageView";

export function UpdateProgressLayer() {
  const { status, latestVersion, error, toastOpen, installing, dismissUpdateToast, applyUpdate } = useUpdate();
  const { upsertToast } = useMultiplexer();
  const { blocking, background, retry, title } = updatePresentation(status, installing, latestVersion);
  useEffect(() => {
    if (!toastOpen || !background) return;
    const notice = updateNotice({
      title,
      body: retry ? "The update could not be completed." : "You can keep working. View download and preparation progress in Updates.",
      failed: retry,
      error,
      latestVersion,
    });
    createNoticePublisher({ upsertToast })({
      kind: "toast", id: "app-update", tabId: null, workspaceId: null,
      title: notice.title, detail: notice.body, severity: notice.severity,
      actions: notice.retry ? [{ label: "Retry", run: () => { void applyUpdate(); } }] : undefined,
    });
    dismissUpdateToast();
  }, [toastOpen, background, retry, title, error, latestVersion, upsertToast, dismissUpdateToast, applyUpdate]);
  if (!blocking) return null;
  const stage = status === "restarting" ? "restarting" : "applying";
  return <UpdateScreen view={updateStageView(stage, latestVersion)} />;
}

export function UpdateScreen({ view, contained = false }: { view: RestoreStageView; contained?: boolean }) {
  return (
    <div data-testid="update-screen" className={`${contained ? "absolute" : "fixed"} inset-0 z-[90] bg-[var(--ct-bg)]`}>
      <RestoreStatePanel view={view} pulse />
    </div>
  );
}
