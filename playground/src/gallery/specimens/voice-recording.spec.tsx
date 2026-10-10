import { useEffect, useMemo, useRef } from "react";
import { ComposerActionButton } from "../../../../src/components/Terminal/ComposerActionButton";
import { ComposerAttachButton } from "../../../../src/components/Terminal/ComposerAttachButton";
import { ComposerDock } from "../../../../src/components/Terminal/ComposerDock";
import { ComposerInputShell } from "../../../../src/components/Terminal/ComposerInputShell";
import { voiceHandle, type VoicePhase } from "../voiceFixture";
import { defineSpecimen } from "../types";

const noop = () => undefined;

function VoiceSpecimen({ phase }: { phase: VoicePhase }) {
  const handle = useMemo(() => voiceHandle(phase), [phase]);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (phase !== "playing") return;
    // Muted media may start without a user gesture; the preview UI is identical.
    const audio = root.current?.querySelector("audio");
    if (!audio) return;
    audio.muted = true;
    const play = () => root.current?.querySelector<HTMLButtonElement>('button[aria-label="Play"]')?.click();
    const timer = window.setTimeout(() => {
      if (audio.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA) play();
      else audio.addEventListener("canplay", play, { once: true });
    }, 300);
    return () => {
      window.clearTimeout(timer);
      audio.removeEventListener("canplay", play);
    };
  }, [phase]);
  return <div ref={root} className="gallery-fill" style={{ padding: `${handle.isHold ? 110 : 36}px ${handle.voiceActive ? 40 : 0}px 36px 0` }}>
    <ComposerDock mode="floating">
      <div className="relative flex w-full items-end gap-1.5">
        {!handle.isPreview && <ComposerAttachButton uploading={false} onClick={noop} />}
        <ComposerInputShell voiceUi={handle}>
          <textarea aria-label="Message composer" className="block w-full min-h-5 resize-none bg-transparent py-0 pr-1 text-[14px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)]" rows={1} placeholder="Message" readOnly />
        </ComposerInputShell>
        {!handle.isPreview && <ComposerActionButton voiceUi={handle} sendDisabled onSend={noop} />}
      </div>
    </ComposerDock>
  </div>;
}

export default defineSpecimen({
  id: "voice-recording",
  title: "Voice recording",
  category: "Voice",
  stage: "wide",
  where: "src/components/Terminal/voice/VoiceRecordingBar.tsx · VoicePreview.tsx · ComposerActionButton.tsx",
  notes: "The shipped composer pieces (ComposerInputShell, VoiceRecordingBar, VoicePreview, ComposerActionButton) driven by a fixture voice handle instead of the microphone, so every phase of the capture flow holds still. Preview plays a generated tone.",
  Component: VoiceSpecimen,
  states: [
    { id: "idle", label: "Idle · mic", props: { phase: "idle" } },
    { id: "recording", label: "Pressed · recording", props: { phase: "hold" } },
    { id: "sliding", label: "Sliding to cancel", props: { phase: "sliding" } },
    { id: "cancel", label: "Cancel armed", props: { phase: "cancel" } },
    { id: "locked", label: "Locked · hands-free", props: { phase: "locked" } },
    { id: "preview", label: "Preview · paused", props: { phase: "preview" } },
    { id: "playing", label: "Preview · playing", props: { phase: "playing" } },
  ],
});
