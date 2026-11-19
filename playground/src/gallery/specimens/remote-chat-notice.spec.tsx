import { useState, type CSSProperties } from "react";
import { RemoteChatNotice, RuntimeTranscriptNotice } from "../../../../src/components/Terminal/TranscriptNotice";
import { REMOTE_CHAT_COPY, type RemoteChatState } from "../../../../src/lib/remoteChatState";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { defineSpecimen } from "../types";

const LIGHT_THEME = themeToCSS({
  ...DEFAULT_THEME,
  name: "github-light",
  background: "#f6f8fa",
  foreground: "#1f2328",
  blue: "#0969da",
  bright_black: "#57606a",
});

function RemoteChatStage({ state, cached = false, details = false, loading = false, empty = false, light = false }: {
  state?: RemoteChatState;
  cached?: boolean;
  details?: boolean;
  loading?: boolean;
  empty?: boolean;
  light?: boolean;
}) {
  const [expanded, setExpanded] = useState(details);
  const [retrying, setRetrying] = useState(loading);
  return <div className={`gallery-fill gallery-transcript bg-[var(--ct-bg)] text-[var(--ct-fg)] ${empty ? "flex min-h-48 flex-col justify-center" : ""}`}
    style={light ? { ...LIGHT_THEME, colorScheme: "light" } as CSSProperties : undefined}
    data-ct-theme={light ? "light" : undefined} role="log" aria-label="Conversation">
    {cached && <p className="mx-auto mb-4 max-w-4xl">I saved the review notes before closing the session.</p>}
    {state ? <RemoteChatNotice state={state} machine="ROG" peerId="cea6e137-aaaa-bbbb-cccc-dddddddddddd"
      failure={state === "not-paired" ? { code: "peer_not_paired", raw: "", path: "/api/mesh/relay-to", status: 404, at: new Date(2026, 10, 18, 14, 32).getTime() } : undefined}
      expanded={expanded} busy={retrying}
      onToggle={() => setExpanded(value => !value)} onRetry={() => setRetrying(true)} onOpenMesh={() => {}} />
      : <RuntimeTranscriptNotice sentence="The agent runtime needs to reconnect." details="The runtime connection closed."
        action={{ label: "Try again", run: () => setRetrying(true), loading: retrying }} />}
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
    { id: "empty", label: "Empty transcript", props: { state: "not-paired", empty: true } },
    { id: "empty-light", label: "Empty transcript · light", props: { state: "not-paired", empty: true, light: true } },
    { id: "cached-details-light", label: "Cached transcript with details · light", props: { state: "not-paired", cached: true, details: true, light: true } },
    { id: "retrying", label: "Retrying", props: { state: "unreachable", cached: true, loading: true } },
    { id: "runtime", label: "Runtime recovery", props: {} },
  ],
});
