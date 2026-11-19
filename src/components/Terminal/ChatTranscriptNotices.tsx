import { useRef, type ReactNode } from "react";
import { DeferredPresence } from "../shared/DeferredPresence";
import type { Tab } from "../../lib/types";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import type { TabFailure } from "../../lib/tabFailure";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import { TabFailureNotice } from "./TabFailureNotice";
import { RemoteChatNotice } from "./TranscriptNotice";
import type { useRemoteChatNotice } from "./useRemoteChatNotice";

export function ChatTranscriptNotices({ tab, workspaceId, remote, retrySend, retryAttachment, attaching, runtime,
  launchedFailure, starting, showRuntime }: {
  tab: Tab | null | undefined;
  workspaceId: string | null;
  remote: ReturnType<typeof useRemoteChatNotice>;
  retrySend: (id: string) => Promise<void>;
  retryAttachment: () => void;
  attaching: boolean;
  runtime: { state: AgentRuntimeState | null; retry: () => Promise<void> };
  launchedFailure: TabFailure | null;
  starting: boolean;
  showRuntime: boolean;
}) {
  const remoteNotice = remote.state && remote.peerId ? <RemoteChatNotice
    state={remote.state} machine={remote.machine} peerId={remote.peerId} failure={remote.failure}
    canRetrySend={remote.canRetrySend} busy={remote.checking || attaching}
    expanded={remote.expanded} onToggle={remote.toggle}
    onRetry={() => {
      if (remote.pendingSend) void remote.checkDelivery(retrySend);
      else retryAttachment();
    }}
    onOpenMesh={() => window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "mesh" } }))}
  /> : null;
  const shown = useRef<ReactNode>(null);
  if (remoteNotice) shown.current = remoteNotice;
  const localNotice = tab && launchedFailure ? <TabFailureNotice tab={tab} failure={launchedFailure} workspaceId={workspaceId}
    detail={tab.boot_failure_reason ?? runtime.state?.reason ?? null} />
    : showRuntime ? <AgentRuntimeBanner tabId={tab?.id} state={runtime.state} onRetry={runtime.retry} startupOwned={starting} /> : null;
  return <>
    <DeferredPresence present={Boolean(remoteNotice)}>
      <div inert={!remoteNotice || undefined}>{shown.current}</div>
    </DeferredPresence>
    {!remoteNotice && localNotice}
  </>;
}
