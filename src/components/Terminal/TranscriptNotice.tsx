import { useState } from "react";
import { ChatNoticeCard } from "@codeterm/chat-ui";
import { Network } from "lucide-react";
import { Button } from "../shared/Button";
import { REMOTE_CHAT_COPY, type RemoteChatFailure, type RemoteChatState } from "../../lib/remoteChatState";

export interface NoticeAction {
  label: string;
  run: () => void;
  loading?: boolean;
}

export function TranscriptNotice({ testId, sentence, details, action, centered, mesh = false, expanded, onToggle }: {
  testId: string;
  sentence: string;
  details?: string;
  action?: NoticeAction;
  centered?: boolean;
  mesh?: boolean;
  expanded: boolean;
  onToggle: () => void;
}) {
  const card = <ChatNoticeCard testId={testId} title={sentence} role="status" quiet
    icon={mesh ? <Network size={16} aria-hidden="true" /> : null}
    expanded={expanded} onToggle={onToggle} detailsLabel="Details"
    status={action && <Button variant="ghost" size="sm" loading={action.loading} onClick={action.run}>{action.label}</Button>}>
    <pre className="whitespace-pre-wrap break-words text-[11px] text-[var(--ct-muted-fg)]">{details}</pre>
    <Button variant="ghost" size="compact" onClick={() => { if (details) void navigator.clipboard?.writeText(details).catch(() => {}); }}>Copy details</Button>
  </ChatNoticeCard>;
  return centered ? <div className="flex min-h-full items-center justify-center">{card}</div> : card;
}

export function RemoteChatNotice({ state, machine, peerId, failure, centered, canRetrySend, busy, expanded, onToggle, onRetry, onOpenMesh }: {
  state: RemoteChatState;
  machine: string;
  peerId: string;
  failure?: RemoteChatFailure;
  centered?: boolean;
  canRetrySend?: boolean;
  busy?: boolean;
  expanded: boolean;
  onToggle: () => void;
  onRetry: () => void;
  onOpenMesh: () => void;
}) {
  const copy = REMOTE_CHAT_COPY[state];
  const details = [
    `Machine: ${machine} (${peerId.slice(0, 8)})`,
    `Cause: ${failure?.code ?? failure?.raw ?? copy.label}`,
    failure?.path && `Request: ${failure.path}${failure.status ? ` → ${failure.status}` : ""}`,
    failure && `Time: ${new Date(failure.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`,
  ].filter(Boolean).join("\n");
  const action = canRetrySend || copy.action === "retry" ? { label: "Try again", run: onRetry, loading: busy }
    : copy.action === "mesh" ? { label: "Open Mesh", run: onOpenMesh } : undefined;
  return <TranscriptNotice testId="remote-chat-notice" sentence={copy.sentence(machine)} details={details} action={action}
    mesh centered={centered} expanded={expanded} onToggle={onToggle} />;
}

export function RuntimeTranscriptNotice({ sentence, details, action, centered }: {
  sentence: string;
  details?: string;
  action?: NoticeAction;
  centered?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  return <TranscriptNotice testId="runtime-chat-notice" sentence={sentence} details={details} action={action}
    centered={centered} expanded={expanded} onToggle={() => setExpanded(value => !value)} />;
}
