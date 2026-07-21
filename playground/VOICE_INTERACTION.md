# Voice recording interaction preservation

This artifact records the product behavior of the chat voice note before any design-system port. The design-system component is intentionally microphone-free; product capture, upload, and transcription remain product responsibilities.

## State machine

| State | Visible contract | Entered by | Exits |
| --- | --- | --- | --- |
| `idle` | 40px mic action; normal composer input | initial state, failed start, discard/cancel/send completion | pointer down starts `starting` |
| `starting` | Mic request is pending; press remains owned by one pointer | `POINTER_DOWN` records start position and captures the pointer | `START_OK` -> `hold` / `locked`; `START_FAIL` -> `idle` |
| `hold` | 88px accent bubble with red mic; input is replaced by live dot, precise timer, waveform, and cancel hint | successful recorder start while pointer remains down | move up -> `locked`; release -> send or cancel; pointer cancel -> `locked` |
| `locked` | Hands-free recorder; stop-square button, Cancel text action, input still replaced | upward gesture or pointer cancellation while holding | tap stop -> recorder finalizes to `preview` (or `idle` without usable audio); Cancel -> `idle` |
| `preview` | Delete, play/pause, played-progress waveform, remaining duration, Send | locked stop produces a usable local clip | Delete -> `idle`; Send uploads and then -> `idle` |
| `busy` flag | Disables duplicate action while media stop/upload is pending; overlays `hold`, `locked`, or `preview` | release, cancel, stop-to-preview, or send-preview begins async recorder work | success chooses `idle` or `preview` |

The reducer is the source of truth: `VoicePhase` and its actions are declared in [`voiceFsm.ts:3`](../src/components/Terminal/voice/voiceFsm.ts:3) and [`voiceFsm.ts:15`](../src/components/Terminal/voice/voiceFsm.ts:15). The initial state and transition implementation are in [`voiceFsm.ts:28`](../src/components/Terminal/voice/voiceFsm.ts:28) and [`voiceFsm.ts:55`](../src/components/Terminal/voice/voiceFsm.ts:55).

## Gesture transitions

1. **Press and hold.** Pointer down only works from `idle`; it stores pointer id and origin, then captures the pointer before requesting the mic ([`useComposerVoice.ts:85`](../src/components/Terminal/voice/useComposerVoice.ts:85)). A successful start advances to `hold`; if the pointer was released while start was pending, the clip is immediately finalized and sent ([`useComposerVoice.ts:95`](../src/components/Terminal/voice/useComposerVoice.ts:95)).
2. **Keep holding.** Pointer moves are listened for on both the button and `window`, so the gesture remains intact beyond the bubble ([`useComposerVoice.ts:112`](../src/components/Terminal/voice/useComposerVoice.ts:112), [`useComposerVoice.ts:157`](../src/components/Terminal/voice/useComposerVoice.ts:157)). The bubble follows with 46% drag follow, elastic limits of 48px horizontal / 56px vertical, and squash/stretch limits ([`composerBubble.tsx:3`](../src/components/Terminal/composerBubble.tsx:3), [`composerBubble.tsx:30`](../src/components/Terminal/composerBubble.tsx:30)).
3. **Slide/flick up to lock.** Moving upward more than 48px, provided up-distance is at least left-distance, transitions `hold` to `locked`; releasing after this is deliberately a no-op ([`voiceFsm.ts:26`](../src/components/Terminal/voice/voiceFsm.ts:26), [`voiceFsm.ts:71`](../src/components/Terminal/voice/voiceFsm.ts:71), [`useComposerVoice.ts:169`](../src/components/Terminal/voice/useComposerVoice.ts:169)). A pointer cancel also locks an active hold so capture is not lost ([`voiceFsm.ts:90`](../src/components/Terminal/voice/voiceFsm.ts:90), [`useComposerVoice.ts:150`](../src/components/Terminal/voice/useComposerVoice.ts:150)).
4. **Slide left or enter cancel zone.** More than 36px left, or entering the input/cancel rectangle, arms cancellation; release then discards instead of sending ([`voiceFsm.ts:30`](../src/components/Terminal/voice/voiceFsm.ts:30), [`useComposerVoice.ts:54`](../src/components/Terminal/voice/useComposerVoice.ts:54), [`useComposerVoice.ts:182`](../src/components/Terminal/voice/useComposerVoice.ts:182)). The visible button changes from red mic to red close.
5. **Release normally.** Releasing `hold` with no cancellation finalizes and sends the voice attachment; cancellation finalizes with discard ([`useComposerVoice.ts:72`](../src/components/Terminal/voice/useComposerVoice.ts:72), [`useComposerVoice.ts:80`](../src/components/Terminal/voice/useComposerVoice.ts:80)).
6. **Locked review.** Tapping the locked stop-square stops the recorder but retains a usable clip locally; Delete drops its object URL, and Send uploads then sends it as an attachment ([`useComposerVoice.ts:122`](../src/components/Terminal/voice/useComposerVoice.ts:122), [`useComposerVoice.ts:130`](../src/components/Terminal/voice/useComposerVoice.ts:130), [`useComposerVoice.ts:139`](../src/components/Terminal/voice/useComposerVoice.ts:139)). Preview UI owns audio playback, scrub progress, and end reset ([`VoicePreview.tsx:24`](../src/components/Terminal/voice/VoicePreview.tsx:24), [`VoicePreview.tsx:34`](../src/components/Terminal/voice/VoicePreview.tsx:34)).

## Timing and motion contract

- Recorder starts MediaRecorder chunks every 250ms; stop requests a trailing 250ms before finalization. Warmup constants are 150ms start and 30 seconds cached-stream idle cleanup ([`useVoiceRecorder.ts:19`](../src/components/Terminal/voice/useVoiceRecorder.ts:19), [`useVoiceRecorder.ts:362`](../src/components/Terminal/voice/useVoiceRecorder.ts:362), [`useVoiceRecorder.ts:382`](../src/components/Terminal/voice/useVoiceRecorder.ts:382)).
- Live timer is updated every 50ms and renders `m:ss,cc`; waveform bar height/opacity transitions are 80ms ([`VoiceRecordingBar.tsx:4`](../src/components/Terminal/voice/VoiceRecordingBar.tsx:4), [`VoiceRecordingBar.tsx:29`](../src/components/Terminal/voice/VoiceRecordingBar.tsx:29), [`VoiceRecordingBar.tsx:60`](../src/components/Terminal/voice/VoiceRecordingBar.tsx:60)).
- Recording/preview replace the input in an inset layer, while attachment and action controls disappear in preview ([`ComposerInputShell.tsx:35`](../src/components/Terminal/ComposerInputShell.tsx:35), [`ComposerActions.tsx:61`](../src/components/Terminal/ComposerActions.tsx:61)).
- The action shell and expanded bubble use 320ms spring-like size transitions; bubble pointer follow is 90ms. Expanded size is 88px ([`globals.css:298`](../src/styles/globals.css:298), [`ComposerActionButton.tsx:154`](../src/components/Terminal/ComposerActionButton.tsx:154)).
- Voice content fades up 4px over 160ms; dot pulses every 1.2s; cancel nudges left every 1.4s; lock pill enters over 200ms and chevron bounces every 1.1s ([`globals.css:248`](../src/styles/globals.css:248), [`globals.css:256`](../src/styles/globals.css:256), [`globals.css:264`](../src/styles/globals.css:264), [`globals.css:272`](../src/styles/globals.css:272), [`globals.css:280`](../src/styles/globals.css:280)).
- Reduced motion removes the ambient dot, cancel, chevron, and bubble effects ([`globals.css:288`](../src/styles/globals.css:288)). The DS port carries the same preference guard in its self-contained stylesheet.

## Preservation risks

- Do not collapse `starting` into `hold`: early pointer release and early upward drag are intentionally remembered while mic permission/start resolves.
- Do not use pointer-up as a stop after lock: it must remain hands-free.
- Do not replace either threshold with a single direction check: vertical lock has precedence only when it clears 48px and is at least as large as left distance; cancellation is 36px left or a geometric drop zone.
- Do not treat preview as a cosmetic state: it owns a retained local blob, delete semantics, upload-on-send, playback progress, and the usable-audio gate.

<!-- revision: 1 -->
