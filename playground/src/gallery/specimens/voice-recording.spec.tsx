import { VoiceRecording, type VoiceRecordingProps } from "../../ds/VoiceRecording";
import type { Specimen } from "../types";

function VoiceRecordingSpecimen(props: VoiceRecordingProps) {
  return <VoiceRecording {...props} />;
}

export default {
  id: "voice-recording",
  title: "Voice recording",
  category: "Voice",
  where: "src/components/Terminal/voice + ComposerActionButton.tsx",
  notes: "A dedicated Voice category keeps this multi-step capture flow together: the board preserves every product state at once, while the inspector exposes its shipped ambient and entry motion.",
  states: [
    { id: "idle", label: "Idle · mic", props: { state: "idle" } },
    { id: "recording", label: "Pressed · recording", props: { state: "recording", elapsed: "0:08,42" } },
    { id: "sliding", label: "Sliding to cancel", props: { state: "sliding", elapsed: "0:08,42" } },
    { id: "locked", label: "Locked · hands-free", props: { state: "locked", elapsed: "0:08,42" } },
    { id: "preview", label: "Preview · playback", props: { state: "preview", playing: true, progress: 0.42 } },
    { id: "cancel", label: "Cancel armed", props: { state: "cancel", elapsed: "0:08,42" } },
  ],
  Component: VoiceRecordingSpecimen,
  motion: "animated",
} satisfies Specimen;
