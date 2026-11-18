import { useState } from "react";
import { RemoteChatNotice } from "../../../../src/components/Terminal/TranscriptNotice";
import { REMOTE_CHAT_COPY, type RemoteChatState } from "../../../../src/lib/remoteChatState";
import { defineSpecimen } from "../types";

function RemoteChatStage({ state, cached = false, details = false, loading = false }: {
  state: RemoteChatState;
  cached?: boolean;
  details?: boolean;
  loading?: boolean;
}) {
  const [expanded, setExpanded] = useState(details);
  const [retrying, setRetrying] = useState(loading);
  return <div className="gallery-fill gallery-transcript p-5" role="log" aria-label="Conversation">
    {cached && <p className="mx-auto mb-4 max-w-4xl">I saved the review notes before closing the session.</p>}
    <RemoteChatNotice state={state} machine="ROG" peerId="cea6e137-aaaa-bbbb-cccc-dddddddddddd"
      failure={state === "not-paired" ? { code: "peer_not_paired", raw: "", path: "/api/mesh/relay-to", status: 404, at: new Date(2026, 10, 18, 14, 32).getTime() } : undefined}
      centered={!cached} expanded={expanded} busy={retrying}
      onToggle={() => setExpanded(value => !value)} onRetry={() => setRetrying(true)} onOpenMesh={() => {}} />
  </div>;
}

export default defineSpecimen({
  id: "remote-chat-notice",
  title: "Remote chat notices",
  category: "Chat",
  where: "src/components/Terminal/TranscriptNotice.tsx",
  stage: "wide",
  notes: "The same card sits in the center of an empty transcript or follows cached messages.",
  Component: RemoteChatStage,
  states: [
    ...Object.entries(REMOTE_CHAT_COPY).map(([state, copy]) => ({
      id: state, label: copy.label, props: { state: state as RemoteChatState },
    })),
    { id: "cached-details", label: "Cached transcript with details", props: { state: "not-paired", cached: true, details: true } },
    { id: "retrying", label: "Retrying", props: { state: "unreachable", cached: true, loading: true } },
  ],
});
