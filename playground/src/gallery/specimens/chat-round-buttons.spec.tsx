import { useEffect, useMemo, useState } from "react";
import { ComposerActionButton } from "../../../../src/components/Terminal/ComposerActionButton";
import { ComposerAttachButton } from "../../../../src/components/Terminal/ComposerAttachButton";
import { ComposerScheduleButton } from "../../../../src/components/Terminal/ComposerScheduleButton";
import { ChatBottomBar } from "../ChatBottomBar";
import { agentTab } from "../fixtures";
import { ForcePseudo, type PseudoState } from "../stageTools";
import { defineSpecimen } from "../types";
import { voiceHandle, type VoicePhase } from "../voiceFixture";

const noop = () => undefined;
const BLOOM_BEAT_MS = 1600;

type ButtonSpecimenProps = {
  kind: "attach" | "action" | "schedule" | "composer";
  send?: boolean;
  bloom?: boolean;
  disabled?: boolean;
  uploading?: boolean;
  phase?: VoicePhase;
  pseudo?: PseudoState;
  draft?: string;
};

function useBloom(enabled: boolean, initial: boolean): boolean {
  const [send, setSend] = useState(initial);
  useEffect(() => {
    if (!enabled) return;
    const timer = window.setInterval(() => setSend((value) => !value), BLOOM_BEAT_MS);
    return () => window.clearInterval(timer);
  }, [enabled]);
  return send;
}

function ActionButton({ send, phase = "idle", disabled = false }: { send: boolean; phase?: VoicePhase; disabled?: boolean }) {
  const handle = useMemo(() => voiceHandle(phase, send), [phase, send]);
  return <ComposerActionButton voiceUi={handle} sendDisabled={disabled} onSend={noop} onSchedule={noop} />;
}

function ButtonSpecimen({ kind, send = false, bloom = false, disabled = false, uploading = false, phase, pseudo, draft }: ButtonSpecimenProps) {
  const bloomSend = useBloom(bloom, send);
  if (kind === "composer") return <ChatBottomBar tab={agentTab(`round-${draft ? "draft" : "empty"}`, { title: "Composer" })} draft={draft} showStatus={false} />;
  const button = kind === "attach"
    ? <ComposerAttachButton uploading={uploading} onClick={noop} />
    : kind === "schedule"
      ? <ComposerScheduleButton onSchedule={noop} />
      : <ActionButton send={bloom ? bloomSend : send} phase={phase} disabled={disabled} />;
  const row = <div className="gallery-row" style={phase && phase !== "idle" ? { padding: "96px 0 48px" } : undefined}>{button}</div>;
  return pseudo ? <ForcePseudo selector="button" state={pseudo}>{row}</ForcePseudo> : row;
}

export default defineSpecimen({
  id: "chat-round-buttons",
  title: "Chat round buttons",
  category: "Buttons",
  stage: "wide",
  where: "src/components/Terminal/ComposerActionButton.tsx · ComposerAttachButton.tsx · ComposerScheduleButton.tsx",
  notes: "The shipped composer controls. Typing turns the mic into send with the accent bloom; the bloom stage flips the real button's send state on a beat. Hover and pressed reuse the product :hover / :active rules. The composer stages render the real ChatComposer.",
  Component: ButtonSpecimen,
  states: [
    { id: "attach", label: "Attach · rest", props: { kind: "attach" } },
    { id: "attach-hover", label: "Attach · hover", props: { kind: "attach", pseudo: "hover" } },
    { id: "attach-uploading", label: "Attach · uploading", props: { kind: "attach", uploading: true } },
    { id: "mic", label: "Voice · rest", props: { kind: "action" } },
    { id: "bloom", label: "Mic → send bloom (loops)", props: { kind: "action", bloom: true } },
    { id: "send", label: "Send · bloomed", props: { kind: "action", send: true } },
    { id: "send-pressed", label: "Send · pressed", props: { kind: "action", send: true, pseudo: "active" } },
    { id: "send-disabled", label: "Send · disabled", props: { kind: "action", send: true, disabled: true } },
    { id: "schedule", label: "Inner · schedule", props: { kind: "schedule" } },
    { id: "recording", label: "Voice · recording", props: { kind: "action", phase: "hold" } },
    { id: "composer", label: "Composer · empty", props: { kind: "composer" } },
    { id: "composer-draft", label: "Composer · draft", props: { kind: "composer", draft: "Ship the gallery update" } },
  ],
});
