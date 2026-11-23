import { createElement, type ReactNode } from "react";
import { TimeProvider, type Scene } from "../engine";

const CONTRACT = {
  "team-glimpse": 2.5,
  "team-return": 8,
  "plan-board": 6,
  jobs: 5,
  "chat-automation": 5,
  browser: 4,
  plugins: 5,
  "theme-repaint": 2.5,
  "opener-dots": 3,
  "mesh-hero": 8,
  "mesh-reach": 6,
  "mesh-swarm": 16.6,
  "browser-site": 8,
  "mesh-memory": 6,
  "memory-graph": 8,
  "end-card": 3,
  blueprint: 8,
  "full-interface": 10,
  neighbors: 28,
  "lab-memory-hook": 7,
  "lab-memory-origin": 11,
  "lab-dreaming": 11,
  "lab-night-shift": 8,
  "lab-fan-out": 7,
  "mob-macro-write": 6,
  "mob-macro-saved": 6,
  "mob-macro-graph": 6,
  "mob-macro-recall": 6,
  "mob-pulse-arrivals": 6,
  "mob-pulse-live": 6,
  "mob-pulse-roundtrip": 6,
  "mob-pulse-remote": 6,
  "mob-phone-open": 6,
  "mob-phone-reply": 6,
  "mob-phone-answer": 6,
  "mob-phone-sync": 6,
  "mob-flow-request": 6,
  "mob-flow-team": 6,
  "mob-flow-lane": 6,
  "mob-flow-report": 6,
  "mob-flow-merge": 6,
  "cx-story-toldit-chat": 5,
  "cx-story-toldit-team": 6,
  "cx-story-toldit-render": 4,
  "cx-story-toldit-review": 6,
  "cx-story-toldit-merge": 4,
  "cx-story-toldit-payoff": 4,
  "cx-story-claude-codex-hook": 4,
  "cx-story-claude-codex-lanes": 7,
  "cx-story-claude-codex-reports": 5,
  "cx-story-claude-codex-review": 6,
  "cx-story-claude-codex-payoff": 4,
  "cx-story-machines-safe-point": 5,
  "cx-story-machines-confirm": 6,
  "cx-story-machines-update": 4,
  "cx-story-machines-restored": 6,
  "cx-story-machines-cooling": 5,
  "cx-story-machines-owner": 6,
  "cx-story-translation-hook": 6,
  "cx-story-translation-lanes": 8,
  "cx-story-translation-logic": 6,
  "cx-story-translation-fonts": 7,
  "cx-story-translation-release": 6,
} as const;

export type SceneId = keyof typeof CONTRACT;

export interface Cut {
  id: string;
  start: number;
  end: number;
  from: number;
}

type Windowed = Scene & { from?: number };

/** Seconds each scene needs before its main content is on screen after a hard cut. */
export const HEAD: Partial<Record<SceneId, number>> = {
  "mesh-reach": 0.6,
  "mesh-memory": 0.4,
  "chat-automation": 0.8,
  "team-return": 0.4,
  "end-card": 0.6,
};

declare global {
  interface Window {
    __filmCuts?: Record<string, Cut[]>;
  }
}

// Scenes load on demand, one chunk each, found by file name (scenes/**/<id>.scene.tsx): a page that plays one reel never
// downloads or evaluates the other reels' scenes, and a reel's later cuts arrive while its first plays. A host that
// already holds every scene module (the playground's film page) registers them all up front with registerScenes.
type SceneModule = { default: Scene | Scene[] };
const loaders = import.meta.glob<SceneModule>("../scenes/**/*.scene.tsx");
const loaderById = new Map(Object.entries(loaders).map(([path, load]) => [path.slice(path.lastIndexOf("/") + 1).replace(/\.scene\.tsx$/, ""), load]));
const registry = new Map<string, Scene>();
const pending = new Map<string, Promise<void>>();
const listeners = new Set<() => void>();
let inFlight = 0;
/** The last frame any loaded scene drew: what a cut still on its way shows instead of an empty frame. */
let lastFrame: ReactNode = null;

/** Adds already-loaded scene modules to the registry. */
export function registerScenes(modules: Record<string, SceneModule> | SceneModule[]) {
  for (const mod of Object.values(modules)) {
    for (const s of [mod.default].flat()) {
      const contract = (CONTRACT as Record<string, number>)[s.id];
      if (contract !== undefined && contract !== s.duration) console.warn(`film: scene ${s.id} lasts ${s.duration}s, its contract says ${contract}s`);
      registry.set(s.id, { ...s, render: (t) => (lastFrame = s.render(t)) });
    }
  }
  for (const listener of listeners) listener();
}

/** Loads the scene with this id (its file's name) once; resolves at once when it is in or unknown. */
export function loadScene(id: string): Promise<void> {
  if (registry.has(id)) return Promise.resolve();
  const load = loaderById.get(id);
  if (!load) return Promise.resolve();
  let job = pending.get(id);
  if (!job) {
    inFlight++;
    job = load()
      .then((mod) => registerScenes([mod]), (error) => console.warn(`film: scene ${id} failed to load`, error))
      .finally(() => inFlight--);
    pending.set(id, job);
  }
  return job;
}

/** Fetches every scene given at once, so none can fall behind the playhead (or a seek). */
export function preloadScenes(ids: readonly string[]): Promise<void> {
  return Promise.all(ids.map(loadScene)).then(() => undefined);
}

/** True while any scene chunk is still on its way: its download and evaluation make frames slow that aren't the film's. */
export const scenesLoading = () => inFlight > 0;

/** Calls back whenever scenes arrive, so a paused frame can redraw with the real scene. */
export function onScenesLoaded(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Stands in for a scene until it arrives: same duration (the contract). Meanwhile it holds the last frame the film drew,
 * never an empty one; only with nothing drawn yet does it show the page background.
 */
function pendingScene(id: SceneId): Scene {
  return {
    id,
    duration: CONTRACT[id],
    render: (t) => {
      const real = registry.get(id);
      if (real) return real.render(t);
      void loadScene(id);
      return lastFrame ?? createElement("div", { "data-film-loading": id, style: { position: "absolute", inset: 0, background: "var(--ct-bg)" } });
    },
  };
}

function placeholder(id: SceneId): Scene {
  return {
    id,
    duration: CONTRACT[id],
    render: () =>
      createElement(
        "div",
        {
          "data-film-missing": id,
          style: {
            position: "absolute",
            inset: 0,
            display: "grid",
            placeItems: "center",
            background: "var(--ct-bg)",
            color: "var(--ct-fg)",
            fontFamily: "var(--ct-font-mono, monospace)",
            fontSize: 48,
          },
        },
        `MISSING SCENE ${id}`,
      ),
  };
}

export const scene = (id: SceneId): Scene => registry.get(id) ?? (loaderById.has(id) ? pendingScene(id) : placeholder(id));

/** A scene as a reel plays it: head trimmed to its first populated frame, optionally cut at `to`. */
export const shot = (id: SceneId, to?: number): Scene => trim(scene(id), HEAD[id] ?? 0, to);

/** Plays `s` from `from` to `to` seconds of its own time; same visuals, shorter window. */
export function trim(s: Scene, from: number, to = s.duration): Scene {
  const base = (s as Windowed).from ?? 0;
  const out: Windowed = {
    id: s.id,
    duration: Math.round((to - from) * 1000) / 1000,
    from: base + from,
    render: (t) => createElement(TimeProvider, { t: t + from, children: s.render(t + from) }),
  };
  return out;
}

/** Freezes the last frame of `s` for `extra` more seconds. */
export function hold(s: Scene, extra: number): Scene {
  const out: Windowed = {
    id: s.id,
    duration: s.duration + extra,
    from: (s as Windowed).from ?? 0,
    render: (t) => {
      const at = Math.min(t, s.duration);
      return createElement(TimeProvider, { t: at, children: s.render(at) });
    },
  };
  return out;
}

/** Plays `s` slower so it lasts `duration` seconds; motion stays alive instead of freezing. */
export function stretch(s: Scene, duration: number): Scene {
  if (duration <= s.duration) return s;
  const k = s.duration / duration;
  const out: Windowed = {
    id: s.id,
    duration: Math.round(duration * 1000) / 1000,
    from: (s as Windowed).from ?? 0,
    render: (t) => createElement(TimeProvider, { t: t * k, children: s.render(t * k) }),
  };
  return out;
}

const round = (v: number) => Math.round(v * 1000) / 1000;

/** Publishes the cut list of a reel for the capture tooling and returns the scenes unchanged. */
export function plan(name: string, scenes: Scene[]): Scene[] {
  let start = 0;
  const cuts = scenes.map((s) => ({ id: s.id, start: round(start), end: round((start += s.duration)), from: (s as Windowed).from ?? 0 }));
  (window.__filmCuts ??= {})[name] = cuts;
  return scenes;
}
