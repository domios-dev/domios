import { useEffect, useState } from "react";
import type { AppConfig } from "../../lib/types";
import { useMuxState } from "../../store/multiplexer/hooks";
import { OPEN_NEW_SESSION_EVENT, type OpenNewSessionDetail } from "../../lib/newSessionChoice";
import { NewSessionPicker } from "./NewSessionPicker";

/** Owns the open session dialog above the shell, so a phone-width sidebar closing or unmounting never takes it away. */
export function NewSessionPickerHost({ firstRunActive, agentsConfig }: { firstRunActive: boolean; agentsConfig?: AppConfig["agents"] }) {
  const state = useMuxState();
  const [request, setRequest] = useState<OpenNewSessionDetail | null>(null);

  useEffect(() => {
    if (firstRunActive) {
      setRequest(null);
      return;
    }
    const open = (event: Event) => {
      const detail = (event as CustomEvent<OpenNewSessionDetail>).detail;
      if (detail?.workspaceId) setRequest(detail);
    };
    window.addEventListener(OPEN_NEW_SESSION_EVENT, open);
    return () => window.removeEventListener(OPEN_NEW_SESSION_EVENT, open);
  }, [firstRunActive]);

  if (!request) return null;
  const isProtected = state.workspaces.find((workspace) => workspace.id === request.workspaceId)?.protected;
  return (
    <NewSessionPicker
      workspaceId={request.workspaceId}
      workspaceCwd={request.workspaceCwd}
      agentsConfig={agentsConfig}
      initialProviderId={request.initialProviderId}
      section={request.section ?? (isProtected ? "general" : "workspace")}
      onClose={() => setRequest(null)}
    />
  );
}
