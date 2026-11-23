import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { shownChars, streamBursts, streamedChars, toolSpan, typingTimes, type Burst } from "./cadence";

/** One step of a tab's scripted beat; `compileBeat` turns the steps into absolute times. */
export type Step =
  | { kind: "type"; id: string; text: string }
  | { kind: "think"; seconds: number }
  | { kind: "say"; id: string; text: string }
  | { kind: "tool"; call: NormalizedChatMessage; result: string; seconds: number; error?: boolean; exitCode?: number }
  | { kind: "arrive"; message: NormalizedChatMessage }
  /** Holds until an absolute story time, so a tab's beat lines up with what another tab does meanwhile. */
  | { kind: "until"; at: number };

type Timed =
  | { kind: "type"; at: number; sendAt: number; id: string; text: string; times: number[] }
  | { kind: "say"; at: number; done: number; id: string; text: string; bursts: Burst[] }
  | { kind: "tool"; at: number; done: number; call: NormalizedChatMessage; result: string; error: boolean; exitCode?: number }
  | { kind: "arrive"; at: number; message: NormalizedChatMessage };

export interface Beat {
  history: NormalizedChatMessage[];
  steps: Timed[];
  /** When the tab starts working; a beat that opens with typing starts once the message is sent. */
  start: number;
  /** When the last step has finished; the beat holds on this frame afterwards. */
  end: number;
}

export interface BeatFrame {
  /** Changes only when something visible changes, so a host re-renders on it, not on every animation frame. */
  key: string;
  messages: NormalizedChatMessage[];
  draft: string;
  working: boolean;
  /** The assistant reply still streaming in, if any. */
  streaming: string | null;
}

const SEND_DELAY = 0.35;
const START = 0.6;

export function compileBeat(history: NormalizedChatMessage[], steps: Step[], seed: number, start = 0): Beat {
  const timed: Timed[] = [];
  let t = start + START;
  steps.forEach((step, index) => {
    const stepSeed = seed * 131 + index;
    if (step.kind === "type") {
      const times = typingTimes(step.text, stepSeed);
      const sendAt = t + (times[times.length - 1] ?? 0) + SEND_DELAY;
      timed.push({ kind: "type", at: t, sendAt, id: step.id, text: step.text, times });
      t = sendAt;
    } else if (step.kind === "think") {
      t += step.seconds;
    } else if (step.kind === "until") {
      t = Math.max(t, step.at);
    } else if (step.kind === "say") {
      const bursts = streamBursts(step.text, stepSeed);
      const done = t + (bursts[bursts.length - 1]?.at ?? 0);
      timed.push({ kind: "say", at: t, done, id: step.id, text: step.text, bursts });
      t = done + 0.25;
    } else if (step.kind === "tool") {
      const done = t + toolSpan(step.seconds);
      timed.push({ kind: "tool", at: t, done, call: step.call, result: step.result, error: !!step.error, exitCode: step.exitCode });
      t = done + 0.3;
    } else {
      timed.push({ kind: "arrive", at: t, message: step.message });
      t += 0.2;
    }
  });
  const first = timed[0];
  const begins = first?.kind === "type" ? first.sendAt : (first?.at ?? t);
  return { history, steps: timed, start: begins, end: t };
}

/** What the tab shows `t` seconds into its beat. */
export function beatFrame(beat: Beat, t: number): BeatFrame {
  const messages = [...beat.history];
  const key: string[] = [];
  let draft = "";
  let streaming: string | null = null;
  for (const step of beat.steps) {
    if (t < step.at) break;
    if (step.kind === "type") {
      if (t < step.sendAt) {
        draft = step.text.slice(0, shownChars(step.times, t - step.at));
        key.push(`d${draft.length}`);
      } else {
        messages.push({ id: step.id, type: "user", content: step.text });
        key.push("u");
      }
    } else if (step.kind === "say") {
      const shown = t >= step.done ? step.text.length : streamedChars(step.bursts, t - step.at);
      messages.push({ id: step.id, type: "assistant", content: step.text.slice(0, shown) });
      if (shown < step.text.length) streaming = step.id;
      key.push(`s${shown}`);
    } else if (step.kind === "tool") {
      const toolId = step.call.toolId ?? step.call.id;
      if (t < step.done) {
        messages.push({ ...step.call, toolId, toolStatus: "running" });
        key.push("r");
      } else {
        const ms = Math.round((step.done - step.at) * 1000);
        messages.push({ ...step.call, toolId, toolStatus: step.error ? "failed" : "completed", toolDurationMs: ms, toolExitCode: step.exitCode, toolError: step.error || undefined });
        messages.push({ id: `${step.call.id}-result`, type: "tool_result", content: step.result, toolResult: step.result, toolId, toolError: step.error || undefined });
        key.push("c");
      }
    } else {
      messages.push(step.message);
      key.push("a");
    }
  }
  const working = t >= beat.start && t < beat.end;
  return { key: `${key.join(",")}|${working ? 1 : 0}`, messages, draft, working, streaming };
}
