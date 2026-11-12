import { useEffect, useRef, useState } from "react";
import type { AppConfig } from "../../lib/types";
import { useMuxState } from "../../store/multiplexer/hooks";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { nativeBridge, type NativeBridge } from "../../lib/nativeBridge";
import { openTemplateFileReview } from "../../lib/teamTemplates/pendingPreview";
import { publishNotice } from "../../lib/notices";
import { NewSessionPicker } from "./NewSessionPicker";

type Workspace = { id: string; cwd: string | null; protected?: boolean };

/** The Team target for an opened package: the active ordinary workspace, else the first one. */
function teamWorkspace(workspaces: readonly Workspace[], activeId: string | null | undefined): Workspace | null {
  const open = workspaces.filter((workspace) => !workspace.protected);
  return open.find((workspace) => workspace.id === activeId) ?? open[0] ?? null;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Owns the open session dialog above the shell, so a phone-width sidebar closing or unmounting never takes it away. */
export function NewSessionPickerHost({ firstRunActive, agentsConfig, openedTeamFiles = nativeBridge.openedTeamFiles }: {
  firstRunActive: boolean;
  agentsConfig?: AppConfig["agents"];
  openedTeamFiles?: NativeBridge["openedTeamFiles"];
}) {
  const state = useMuxState();
  const [request, setRequest] = useState<{ detail: OpenNewSessionDetail; serial: number } | null>(null);
  const latest = useRef(state);
  latest.current = state;

  useEffect(() => {
    if (firstRunActive) {
      setRequest(null);
      return;
    }
    const open = (event: Event) => {
      const detail = (event as CustomEvent<OpenNewSessionDetail>).detail;
      if (detail?.workspaceId) setRequest((current) => ({ detail, serial: (current?.serial ?? 0) + 1 }));
    };
    window.addEventListener(OPEN_NEW_SESSION_EVENT, open);
    return () => window.removeEventListener(OPEN_NEW_SESSION_EVENT, open);
  }, [firstRunActive]);

  // A package waits natively through first run and is taken only once the dialog can show its review.
  useEffect(() => {
    if (firstRunActive || !openedTeamFiles) return;
    let unsubscribe: (() => void) | null = null;
    let live = true;
    const review = (path: string) => {
      const current = latest.current;
      const workspace = teamWorkspace(current.workspaces as Workspace[], current.active_workspace_id);
      if (workspace) openTemplateFileReview(workspace, path);
      else publishNotice({ kind: "toast", title: `Open a workspace to preview ${fileName(path)}`, severity: "Warning" });
    };
    openedTeamFiles.subscribe(review).then(
      (stop) => { if (live) unsubscribe = stop; else stop(); },
      (error: unknown) => console.warn("Opened team files are unavailable", error),
    );
    return () => {
      live = false;
      unsubscribe?.();
    };
  }, [firstRunActive, openedTeamFiles]);

  if (!request) return null;
  const { detail } = request;
  const isProtected = state.workspaces.find((workspace) => workspace.id === detail.workspaceId)?.protected;
  return (
    <NewSessionPicker
      key={request.serial}
      workspaceId={detail.workspaceId}
      workspaceCwd={detail.workspaceCwd}
      agentsConfig={agentsConfig}
      initialProviderId={detail.initialProviderId}
      section={detail.section ?? (isProtected ? "general" : "workspace")}
      onClose={() => setRequest(null)}
    />
  );
}
