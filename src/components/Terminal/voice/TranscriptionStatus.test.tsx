import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { formatTranscriptionElapsed, TranscriptionStatus, useTranscriptionStatus } from "./TranscriptionStatus";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(1_000_000); });
afterEach(() => { vi.useRealTimers(); });

it("formats elapsed time as minutes and seconds without an estimate", () => {
  expect(formatTranscriptionElapsed(0)).toBe("0:00");
  expect(formatTranscriptionElapsed(12_400)).toBe("0:12");
  expect(formatTranscriptionElapsed(65_000)).toBe("1:05");
  expect(formatTranscriptionElapsed(-5)).toBe("0:00");
});

it("shows elapsed transcription time only while a tracked send is pending", async () => {
  let track!: <T>(work: Promise<T>) => Promise<T>;
  function Harness() {
    const status = useTranscriptionStatus();
    track = status.track;
    return <TranscriptionStatus since={status.since} />;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    const status = () => host.querySelector('[role="status"][aria-label="Voice transcription"]');
    expect(status()).toBeNull();
    let finish!: (text: string) => void;
    let tracked!: Promise<string>;
    await act(async () => { tracked = track(new Promise<string>(done => { finish = done; })); });
    expect(status()?.textContent).toBe("Transcribing… 0:00");
    await act(async () => { vi.advanceTimersByTime(12_000); });
    expect(status()?.textContent).toBe("Transcribing… 0:12");
    expect(status()?.textContent).not.toMatch(/%/);
    await act(async () => { finish("composed"); await tracked; });
    expect(status()).toBeNull();
    await act(async () => { await track(Promise.reject(new Error("transcription failed"))).catch(() => {}); });
    expect(status()).toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
});
