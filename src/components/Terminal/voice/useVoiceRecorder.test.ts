import {
  acquireWarmStreamForRecorder,
  releaseWarmStreamUser,
  resetWarmStreamForTest,
  retainWarmStreamUser,
} from "./useVoiceRecorder";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

type FakeTrack = MediaStreamTrack & { stopped: boolean };

function makeStream(): MediaStream & { track: FakeTrack } {
  const track = {
    stopped: false,
    kind: "audio",
    get readyState() {
      return this.stopped ? "ended" : "live";
    },
    stop() {
      this.stopped = true;
    },
  } as FakeTrack;

  return {
    track,
    getAudioTracks: () => [track],
    getTracks: () => [track],
  } as unknown as MediaStream & { track: FakeTrack };
}

async function main() {
  let getUserMediaCalls = 0;
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      mediaDevices: {
        getUserMedia: async () => {
          getUserMediaCalls += 1;
          return makeStream();
        },
      },
    },
  });

  resetWarmStreamForTest();
  retainWarmStreamUser();
  const first = await acquireWarmStreamForRecorder();
  releaseWarmStreamUser();

  retainWarmStreamUser();
  const second = await acquireWarmStreamForRecorder();
  releaseWarmStreamUser();

  assert(first === second, "warm stream survives a recorder remount");
  assert(getUserMediaCalls === 1, "second recording reuses the warm getUserMedia stream");
  assert(second.getAudioTracks()[0]?.readyState === "live", "reused warm stream remains live");

  resetWarmStreamForTest();
  assert(second.getAudioTracks()[0]?.readyState === "ended", "test reset stops the warm stream");
  console.log("useVoiceRecorder warm stream checks passed");
}

main().catch((error) => {
  resetWarmStreamForTest();
  console.error(error);
  process.exit(1);
});
