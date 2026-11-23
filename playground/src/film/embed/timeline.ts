import type { Cut } from "../reels/library";
import { reportAt, REPORT_COUNT } from "../scenes/product/chatTimes";
import { BLUEPRINT_RUN } from "../fixtures/blueprint";
import { GUEST_JOINS } from "../guest";

export interface Chapter { title: string; start: number; end: number }

/** Each chapter opens at the first reel cut among its ids and runs until the next chapter; the first opens the reel. */
export const WEB_HALE_CHAPTERS: [string, string[]][] = [
  ["Memory", ["lab-memory-hook", "lab-memory-origin", "memory-graph"]],
  ["Mesh", ["mesh-hero", "mesh-swarm", "mesh-reach"]],
  ["Neighbors", ["neighbors"]],
  ["Team", ["team-glimpse", "team-return"]],
  ["Blueprints", ["blueprint"]],
  ["Jobs & chat", ["jobs", "chat-automation"]],
  ["Jev", ["lab-night-shift"]],
  ["Browser & plugins", ["browser-site", "browser", "plugins"]],
  ["Interface", ["full-interface"]],
];

/** Cuts drawn by the memory graph, the heaviest live scenes; weak devices may play them from a clip. */
export const MEMORY_GRAPH_CUTS = ["lab-memory-hook", "lab-memory-origin", "memory-graph", "lab-dreaming"];


export function chaptersOf(cuts: Cut[], duration: number, marks = WEB_HALE_CHAPTERS): Chapter[] {
  const starts = marks.flatMap(([title, ids], i) => {
    const cut = cuts.find((c) => ids.includes(c.id));
    if (!cut) return [];
    return [{ title, start: i === 0 ? 0 : cut.start }];
  });
  return starts.map((c, i) => ({ ...c, end: starts[i + 1]?.start ?? duration }));
}


export const chapterAt = (chapters: Chapter[], t: number): Chapter | undefined => [...chapters].reverse().find((c) => t >= c.start) ?? chapters[0];

/** The contiguous run of `ids` that opens with the first of them, or null when none is present. */
export function spanOf(cuts: Cut[], ids: string[]): { start: number; end: number } | null {
  const first = cuts.findIndex((c) => ids.includes(c.id));
  if (first < 0) return null;
  let last = first;
  while (cuts[last + 1] && ids.includes(cuts[last + 1].id)) last++;
  return { start: cuts[first].start, end: cuts[last].end };
}

export const FRAME_SAMPLES = 30;
export const WEAK_FRAME_MS = 24;

export function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/** Undecided until enough frame intervals are in; then weak when their median exceeds the budget. */
export function isWeakDevice(intervals: number[], samples = FRAME_SAMPLES, budgetMs = WEAK_FRAME_MS): boolean | null {
  return intervals.length < samples ? null : median(intervals.slice(0, samples)) > budgetMs;
}

/** The seconds of the reel a scene card loops: `window` inside the cut, clamped to it; null for an unknown cut. */
export function loopWindow(cut: Cut | undefined, window?: [number, number]): { from: number; to: number } | null {
  if (!cut) return null;
  const length = cut.end - cut.start;
  const [a, b] = window ?? [0, length];
  const from = cut.start + Math.max(0, Math.min(a, length));
  const to = cut.start + Math.max(0, Math.min(b, length));
  return to - from > 0.1 ? { from, to } : null;
}

export const LOOP_FADE = 0.4;

/** Opacity across one loop: up from the background at its start, back down before it wraps, so the jump never shows. */
export function loopFade(local: number, length: number, fade = LOOP_FADE): number {
  const edge = Math.min(fade, length / 4);
  return Math.max(0, Math.min(1, local / edge, (length - local) / edge));
}

/** The chat scene starts this far in on the reel, and plays stretched over the rest of its length. */
const CHAT_HEAD = 0.8;
const CHAT_LENGTH = 5;

/** "report:N" stays on from the moment report N lands in the chat until the scene ends, so the page can show every report so far. */
function reportCues(): CueSpan[] {
  return Array.from({ length: REPORT_COUNT }, (_, index) => ({
    cue: `report:${index}`,
    cut: "chat-automation",
    from: Math.max(0, (reportAt(index) - CHAT_HEAD) / (CHAT_LENGTH - CHAT_HEAD)),
    to: 1,
  }));
}

/** The blueprint scene's length; its run timings are in scene seconds and the reel stretches the scene evenly. */
const BLUEPRINT_LENGTH = 8;

/** A named beat the page can answer, active over a share of one cut (0 to 1 of its length). */
export interface CueSpan { cue: string; cut: string; from: number; to: number }

export const WEB_HALE_CUES: CueSpan[] = [
  { cue: "scenic", cut: "lab-memory-hook", from: 0, to: 1 },
  { cue: "scenic", cut: "lab-memory-origin", from: 0, to: 1 },
  { cue: "scenic", cut: "memory-graph", from: 0, to: 1 },
  // While the machines dream the page goes soft around the screen, until the cross-machine recall starts.
  { cue: "dream", cut: "lab-dreaming", from: 0, to: 0.5 },
  // The stars that fell with the dream are pulled back up as it ends.
  { cue: "lift", cut: "lab-dreaming", from: 0.36, to: 0.5 },
  // The opening's one memory bursts into the whole year of memory.
  { cue: "burst", cut: "lab-memory-hook", from: 0.05, to: 0.4 },
  { cue: "memory", cut: "lab-memory-origin", from: 0.22, to: 0.45 },
  { cue: "frame-soft", cut: "memory-graph", from: 0.85, to: 1 },
  { cue: "frame-soft", cut: "lab-dreaming", from: 0, to: 0.08 },
  // The viewer's machine joins the mesh as "one private mesh" lands, and stays until the scene moves on.
  { cue: "guest", cut: "mesh-hero", from: GUEST_JOINS / 8, to: 1 },
  // The team's kanban runs on past the frame while the orchestrator, workers and curator move its cards.
  { cue: "board", cut: "team-return", from: 0.12, to: 0.98 },
  // The Neighbors' own machines sit past the frame, where their encrypted channels start.
  { cue: "neighbors", cut: "neighbors", from: 0.06, to: 0.97 },
  { cue: "frame-soft", cut: "blueprint", from: 0.35, to: 0.95 },
  // The health check fails with a 503 until the diagnose agent's fix lands, then the result is delivered.
  { cue: "incident", cut: "blueprint", from: BLUEPRINT_RUN.checkDown / BLUEPRINT_LENGTH, to: BLUEPRINT_RUN.diagnoseDone / BLUEPRINT_LENGTH },
  { cue: "recovered", cut: "blueprint", from: BLUEPRINT_RUN.diagnoseDone / BLUEPRINT_LENGTH, to: (BLUEPRINT_RUN.resultDelivered + 0.4) / BLUEPRINT_LENGTH },
  { cue: "phone", cut: "chat-automation", from: 0, to: 1 },
  ...reportCues(),
  { cue: "jev", cut: "lab-night-shift", from: 0.02, to: 0.97 },
  { cue: "night", cut: "lab-night-shift", from: 0.08, to: 0.775 },
  { cue: "dawn", cut: "lab-night-shift", from: 0.775, to: 0.9 },
  // From sunrise to the end of the night shift: "N reports while you slept".
  { cue: "morning", cut: "lab-night-shift", from: 0.8, to: 1 },
  // The plugins list reaches Pebble: the site gets the plugin for a moment too.
  { cue: "plugin:pebble", cut: "plugins", from: 2.93 / 5, to: 4.45 / 5 },
  { cue: "spread", cut: "end-card", from: 0.04, to: 0.85 },
];

/** Every cue whose span holds t, in listed order. */
export function cuesAt(cuts: Cut[], t: number, cues = WEB_HALE_CUES): string[] {
  return cues.flatMap(({ cue, cut, from, to }) => {
    const c = cuts.find((x) => x.id === cut);
    if (!c) return [];
    const length = c.end - c.start;
    return t >= c.start + from * length && t < c.start + to * length ? [cue] : [];
  });
}

export const REACH_IN = 1.6;
export const REACH_OUT = 2.4;
const smooth = (k: number) => k * k * (3 - 2 * k);

/** Cues during which the spill reaches far past the frame; each eases in at its start and out at its end. */
const REACH_CUES = ["scenic", "board", "neighbors"];

/** How far the spill reaches: 0 normally, 1 at full reach while a reach cue's span runs (its cuts joined into one span). */
export function reachAt(cuts: Cut[], t: number, cues = WEB_HALE_CUES): number {
  let reach = 0;
  for (const name of REACH_CUES) {
    const spans = cues.filter((c) => c.cue === name).flatMap(({ cut, from, to }) => {
      const c = cuts.find((x) => x.id === cut);
      return c ? [{ start: c.start + from * (c.end - c.start), end: c.start + to * (c.end - c.start) }] : [];
    });
    if (!spans.length) continue;
    const start = Math.min(...spans.map((s) => s.start));
    const end = Math.max(...spans.map((s) => s.end));
    if (t > start && t < end) reach = Math.max(reach, smooth(Math.min(1, (t - start) / REACH_IN, (end - t) / REACH_OUT)));
  }
  return reach;
}
