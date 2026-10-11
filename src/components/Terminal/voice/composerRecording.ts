export interface RecordedVoiceClip {
  blob: Blob;
  mime: string;
}

export interface RecordingRuntime {
  getUserMedia(constraints: MediaStreamConstraints): Promise<MediaStream>;
  createRecorder(stream: MediaStream, mime: string | undefined): MediaRecorder;
}

export interface ComposerRecording {
  start(microphoneId: string, runtime: RecordingRuntime, onCancel: () => void, onStop: () => void): Promise<boolean>;
  stop(discard: boolean): Promise<RecordedVoiceClip | null>;
}

const recorders = new Map<string, ComposerRecording>();

export function registerComposerRecording(tabId: string, recorder: ComposerRecording): () => void {
  recorders.set(tabId, recorder);
  return () => {
    if (recorders.get(tabId) === recorder) recorders.delete(tabId);
  };
}

export function composerRecording(tabId: string): ComposerRecording | undefined {
  return recorders.get(tabId);
}
