import { beatFrame, type Beat, type BeatFrame } from "./beat";

export interface Activity {
  duration: number;
  beats: Readonly<Record<string, Beat>>;
}

export const ACTIVITY_HISTORY_LIMIT = 14;

/** The clock never rolls over. Only the sample data repeats, with fresh message IDs for each run. */
export function activityFrame(scripts: readonly Activity[], tab: string, seconds: number): BeatFrame {
  const cycle = scripts.reduce((sum, script) => sum + script.duration, 0);
  const lap = Math.floor(seconds / cycle);
  const messages = new Map<string, BeatFrame["messages"][number]>();
  const firstLap = Math.max(0, lap - 1);
  let start = firstLap * cycle;
  let current: BeatFrame | undefined;
  for (let run = firstLap; run <= lap; run++) {
    for (const [index, script] of scripts.entries()) {
      if (start > seconds) break;
      const beat = script.beats[tab];
      if (beat) {
        const local = seconds - start;
        const frame = beatFrame(beat, local);
        const suffix = `-${run}-${index}`;
        for (const message of frame.messages) {
          const key = `${index}:${message.id}`;
          // Once an episode returns, keep the newest copy of each scripted event.
          // A tool call and its result are replaced together when the tool starts.
          messages.delete(key);
          if (message.toolStatus === "running") messages.delete(`${key}-result`);
          messages.set(key, { ...message, id: message.id + suffix, toolId: message.toolId && message.toolId + suffix });
        }
        if (local < script.duration) {
          current = { ...frame, streaming: frame.streaming && frame.streaming + suffix };
        }
      }
      start += script.duration;
    }
  }
  const retained = [...messages.values()].slice(-ACTIVITY_HISTORY_LIMIT);
  return {
    key: `activity:${retained.map((message) => message.id).join(",")}:${current?.key ?? ""}`,
    messages: retained,
    working: current?.working ?? false,
    draft: current?.draft ?? "",
    streaming: current?.streaming ?? null,
  };
}
