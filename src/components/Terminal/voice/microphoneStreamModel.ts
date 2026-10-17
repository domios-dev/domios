export interface MicrophoneTrack {
  readyState: string;
  enabled: boolean;
  stop(): void;
}

export interface MicrophoneStream {
  getAudioTracks(): MicrophoneTrack[];
  getTracks(): MicrophoneTrack[];
}

export function createMicrophoneStreamModel<T extends MicrophoneStream>() {
  let stream: T | null = null;
  let pending: Promise<T> | null = null;
  let generation = 0;

  function release() {
    generation += 1;
    stream?.getTracks().forEach((track) => track.stop());
    stream = null;
    pending = null;
  }

  function pause() {
    stream?.getAudioTracks().forEach((track) => { track.enabled = false; });
  }

  function acquire(request: () => Promise<T>): Promise<T> {
    if (stream?.getAudioTracks().some((track) => track.readyState === "live")) {
      stream.getAudioTracks().forEach((track) => { track.enabled = true; });
      return Promise.resolve(stream);
    }
    if (pending) return pending;
    release();
    const operation = generation;
    pending = request().then((captured) => {
      if (operation !== generation) {
        captured.getTracks().forEach((track) => track.stop());
        throw new Error("Microphone request was cancelled when the page was hidden.");
      }
      stream = captured;
      return captured;
    }).finally(() => {
      if (operation === generation) pending = null;
    });
    return pending;
  }

  return { acquire, pause, release };
}
