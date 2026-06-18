import {
  MIN_VOICE_BYTES,
  TEST_AUDIO_MIME,
  installCodeTermTestAudioMocks,
  isCodeTermFrontendTestMode,
  pickAudioMime,
} from "./audio";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: globalThis,
});

Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {},
});

assert(!isCodeTermFrontendTestMode({ DEV: false, VITE_CODETERM_TEST: "1" }), "release ignores test flag");
assert(isCodeTermFrontendTestMode({ DEV: true, VITE_CODETERM_TEST: "1" }), "dev test flag enables test mode");

installCodeTermTestAudioMocks(true);

assert(pickAudioMime() === TEST_AUDIO_MIME, "test mode picks deterministic audio mime");

async function main() {
  window.__gumCalls = 0;
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  assert(window.__codetermTestAudio?.getUserMediaCalls === 1, "test getUserMedia call counted");
  assert(window.__gumCalls === 1, "legacy Playwright getUserMedia counter is preserved");
  assert(stream.getAudioTracks()[0]?.readyState === "live", "fake stream exposes a live audio track");

  const chunks: Blob[] = [];
  const recorder = new MediaRecorder(stream, { mimeType: TEST_AUDIO_MIME });
  recorder.ondataavailable = (event) => chunks.push(event.data);
  await new Promise<void>((resolve) => {
    recorder.onstop = () => resolve();
    recorder.start();
    recorder.requestData();
    recorder.stop();
  });

  assert(chunks.length > 0, "fake recorder emits data");
  assert(chunks[0]!.size > MIN_VOICE_BYTES, "fake recorder emits usable voice bytes");

  console.log("audio test-mode seam checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
