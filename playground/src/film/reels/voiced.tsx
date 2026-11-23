import { sequence, type Scene } from "../engine";
import { plan, shot, stretch, trim, type SceneId } from "./library";

export interface VoicedTiming {
  name: string;
  /** from: seconds of the scene skipped at its head, to open on a moment already under way. */
  cuts: { id: SceneId; duration: number; from?: number }[];
}

/** Fits a scene to `duration`: slower when it needs more time, cut short when the next word arrives sooner. */
function fit(id: SceneId, duration: number, from = 0): Scene {
  const s = from ? trim(shot(id), from) : shot(id);
  return duration < s.duration ? trim(s, 0, duration) : stretch(s, duration);
}

/** A hard-cut reel whose scene lengths come from the narration. */
export function voiced(timing: VoicedTiming): Scene {
  return sequence(timing.name, plan(timing.name, timing.cuts.map((c) => fit(c.id, c.duration, c.from))));
}
