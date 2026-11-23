import "../../../../src/styles/globals.css";
import "./embed.css";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { loadScene, onScenesLoaded, preloadScenes, registerScenes } from "../reels/library";
import webHale from "../reels/web-hale.reel";
import memoryHook from "../scenes/lab/lab-memory-hook.scene";
import { FilmEmbed, type FilmAction, type FilmController, type FilmLayout, type FilmState, type PlayResult } from "./FilmEmbed";
import { SceneCard, type SceneCardController } from "./SceneCard";
import { enterScope } from "./scope";
import { applyFilmTheme, type FilmTheme } from "./theme";
import { ROOT_CLASS } from "./rootClass";
import type { GuideId } from "../workspace/tour";
import type { PlaceId } from "../workspace/nav";

export type { FilmAction, FilmLayout, FilmState, FilmTheme, PlayResult };

export type { PlaceId };

export interface WorkspaceOptions {
  onGuideEnd?: (action: GuideId) => void;
  theme: FilmTheme;
  reducedMotion: boolean;
  /** "portrait" is the app's own narrow phone layout at real type sizes. */
  layout: FilmLayout;
  place: PlaceId;
  /** Fires when a click inside the workspace moves to another place; never for setPlace. */
  onPlace?: (place: PlaceId) => void;
  /** The cursor tour over the app's own controls; on by default, never in portrait or with reduced motion. */
  tour?: boolean;
}

export interface WorkspaceHandle {
  setTheme(theme: FilmTheme): void;
  setLayout(layout: FilmLayout): void;
  /** Moves to a place, keeping what the visitor opened; `replay` restarts the story from that place's beat. */
  setPlace(place: PlaceId, options?: { replay?: boolean }): void;
  /** Starts or stops the cursor tour; the visitor's own input inside the frame stops it too. */
  setTour(on: boolean): void;
  guide(action: GuideId): void;
  /** Viewport rect of the inert composer the page lays its fog over, or null while no chat is shown. */
  anchor(id: "composer"): DOMRect | null;
  pause(): void;
  resume(): void;
  destroy(): void;
}

export interface FilmOptions {
  reel: "web-hale";
  audioUrl: string;
  captionsUrl: string;
  theme: FilmTheme;
  layout: FilmLayout;
  reducedMotion: boolean;
  /** Links the end card offers once the film is over, such as Download and GitHub. */
  actions?: FilmAction[];
  /** The viewer's machine as the page names it ("Your Mac"); it joins the film's mesh. */
  guest?: string;
}

export interface FilmHandle {
  setTheme(theme: FilmTheme): void;
  setLayout(layout: FilmLayout): void;
  /** Replaces the end card's links, for example once the page knows the visitor's platform. */
  setActions(actions: FilmAction[]): void;
  /** Starts without moving focus, with the sound as it is; rejects when the browser refuses sound without a gesture. */
  play(): Promise<PlayResult>;
  pause(): void;
  setMuted(muted: boolean): void;
  /** Sets the narration and music level, 0 to 1. */
  setVolume(volume: number): void;
  /** Hides the controls while playing, for a host that shows the film edge to edge. */
  setBare(bare: boolean): void;
  /** Calls back on every state change, and once right away. */
  subscribe(listener: (state: FilmState) => void): () => void;
  /** Viewport rect of the film element marked data-film-anchor="id" (getBoundingClientRect), or null when none is on screen. */
  anchor(id: string): DOMRect | null;
  destroy(): void;
}

export interface SceneOptions {
  reel: "web-hale";
  /** A cut id of the reel, such as "memory-graph" or "lab-dreaming". */
  cut: string;
  theme: FilmTheme;
  reducedMotion: boolean;
  /** Seconds inside the cut to loop; the whole cut by default. */
  window?: [number, number];
}

export interface SceneHandle {
  setTheme(theme: FilmTheme): void;
  pause(): void;
  resume(): void;
  destroy(): void;
}

const REELS = { "web-hale": webHale };
const STYLESHEET = "film.css";

// The opening cut is the poster and the first seconds, so it ships in this module; every other cut is its own chunk,
// all fetched together the moment the player mounts (see preloadReel), never at page load.
registerScenes([{ default: memoryHook }]);

/** The reel's cut ids in playing order, as its plan published them. */
const cutIds = (reel: { id: string }) => [...new Set((window.__filmCuts?.[reel.id] ?? []).map((c) => c.id))];

/**
 * Fetches all of the reel's other cuts at once as the player mounts (the host mounts it only once the page is idle),
 * so no cut can fall behind the playhead or a seek; redraws as they arrive, which only matters to a paused frame.
 */
function preloadReel(reel: { id: string }, redraw: () => void): () => void {
  const stop = onScenesLoaded(redraw);
  void preloadScenes(cutIds(reel));
  return stop;
}

/** A shadow host under `root` with the film stylesheet and a themed root that stands in for the document. */
function shadowHost(root: HTMLElement, className: string, layout: FilmLayout, theme: FilmTheme) {
  const host = document.createElement("div");
  host.className = className;
  host.dataset.layout = layout;
  const shadow = host.attachShadow({ mode: "open" });
  const sheet = document.createElement("link");
  sheet.rel = "stylesheet";
  sheet.href = new URL(STYLESHEET, import.meta.url).href;
  const filmRoot = document.createElement("div");
  filmRoot.className = ROOT_CLASS;
  filmRoot.hidden = true;
  const reveal = () => { filmRoot.hidden = false; };
  sheet.addEventListener("load", reveal, { once: true });
  sheet.addEventListener("error", reveal, { once: true });
  shadow.append(sheet, filmRoot);
  applyFilmTheme(filmRoot, theme);
  root.append(host);
  return { host, filmRoot, leaveScope: enterScope(filmRoot), reactRoot: createRoot(filmRoot) };
}

/** Mounts one cut of the reel as a silent live loop: no audio, bar or poster; it pauses itself off screen. */
export function mountScene(root: HTMLElement, options: SceneOptions): SceneHandle {
  const { host, filmRoot, leaveScope, reactRoot } = shadowHost(root, "domios-film domios-film-scene", "landscape", options.theme);
  const controller: SceneCardController = { setPaused: () => undefined };
  const render = () => reactRoot.render(createElement(SceneCard, { reel: REELS[options.reel], cut: options.cut, window: options.window, reducedMotion: options.reducedMotion, controller }));
  render();
  // Only this card's cut is needed here; it loads at once, the rest of the reel is left alone.
  const stopLoading = onScenesLoaded(render);
  void loadScene(options.cut);
  let theme = options.theme;
  let destroyed = false;
  return {
    setTheme(next) {
      if (destroyed || next === theme) return;
      theme = next;
      applyFilmTheme(filmRoot, next);
    },
    pause: () => controller.setPaused(true),
    resume: () => controller.setPaused(false),
    destroy() {
      if (destroyed) return;
      destroyed = true;
      stopLoading();
      reactRoot.unmount();
      leaveScope();
      host.remove();
    },
  };
}

/**
 * Mounts one explorable, scripted Domios workspace built from the app's own components. The workspace code is a
 * separate chunk fetched on this call; the handle works at once and its calls apply when the chunk arrives.
 */
export function mountWorkspace(root: HTMLElement, options: WorkspaceOptions): WorkspaceHandle {
  const { host, filmRoot, leaveScope, reactRoot } = shadowHost(root, "domios-film domios-film-workspace", options.layout, options.theme);
  let state = { layout: options.layout, place: options.place, tour: options.tour ?? true };
  let theme = options.theme;
  let destroyed = false;
  // Until the chunk mounts, calls only record state; the workspace reads it when it takes the controller over.
  const controller = {
    paused: false,
    setPlace: (_place: PlaceId) => undefined as void,
    pendingGuide: null as GuideId | null,
    guideFinished: (action: GuideId) => options.onGuideEnd?.(action),
    guide: (action: GuideId) => { controller.pendingGuide = action; },
    setPaused: (value: boolean) => { controller.paused = value; },
    setTour: (_on: boolean) => undefined as void,
    composer: (): HTMLElement | null => null,
  };
  const onPlace = (place: PlaceId) => {
    state = { ...state, place };
    options.onPlace?.(place);
  };
  let render = () => undefined as void;
  void import("./WorkspaceCard").then(({ WorkspaceCard }) => {
    if (destroyed) return;
    render = () => reactRoot.render(createElement(WorkspaceCard, { ...state, reducedMotion: options.reducedMotion, tour: state.tour, controller, onPlace }));
    render();
  });
  return {
    setTheme(next) {
      if (destroyed || next === theme) return;
      theme = next;
      applyFilmTheme(filmRoot, next);
    },
    setLayout(layout) {
      if (destroyed || layout === state.layout) return;
      state = { ...state, layout };
      host.dataset.layout = layout;
      render();
    },
    setPlace(place) {
      if (destroyed) return;
      state = { ...state, place };
      controller.setPlace(place);
    },
    setTour(on) {
      if (destroyed) return;
      state = { ...state, tour: on };
      controller.setTour(on);
    },
    guide(action) {
      if (!destroyed) controller.guide(action);
    },
    anchor() {
      const el = destroyed ? null : controller.composer();
      return el ? el.getBoundingClientRect() : null;
    },
    pause() {
      controller.paused = true;
      controller.setPaused(true);
    },
    resume() {
      controller.paused = false;
      controller.setPaused(false);
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      reactRoot.unmount();
      leaveScope();
      host.remove();
    },
  };
}

/** Mounts the live film into a shadow root under `root`; nothing from the page reaches it and nothing leaks out. */
export function mountFilm(root: HTMLElement, options: FilmOptions): FilmHandle {
  const { host, filmRoot, leaveScope, reactRoot } = shadowHost(root, "domios-film", options.layout, options.theme);
  const listeners = new Set<(state: FilmState) => void>();
  let current: FilmState = { started: false, playing: false, muted: false, ended: false, chapter: null, pausedBy: null, cue: null, cues: [] };
  let ready: () => void = () => undefined;
  const mounted = new Promise<void>((resolve) => { ready = resolve; });
  const controller: FilmController = {
    pause: () => undefined,
    play: () => mounted.then(() => controller.play()),
    ready: () => ready(),
    setMuted: () => undefined,
    setVolume: () => undefined,
    setBare: () => undefined,
    emit: (next) => {
      current = next;
      for (const listener of listeners) listener(next);
    },
  };
  let state = { theme: options.theme, layout: options.layout, actions: options.actions ?? [] };
  const render = () =>
    reactRoot.render(
      createElement(FilmEmbed, {
        scene: REELS[options.reel],
        audioUrl: options.audioUrl,
        captionsUrl: options.captionsUrl,
        reducedMotion: options.reducedMotion,
        guest: options.guest ?? null,
        controller,
        ...state,
      }),
    );
  render();
  const stopLoading = preloadReel(REELS[options.reel], () => render());

  let destroyed = false;
  return {
    setTheme(theme) {
      if (destroyed || theme === state.theme) return;
      state = { ...state, theme };
      applyFilmTheme(filmRoot, theme);
      render();
    },
    setActions(actions) {
      if (destroyed) return;
      state = { ...state, actions };
      render();
    },
    setLayout(layout) {
      if (destroyed || layout === state.layout) return;
      state = { ...state, layout };
      host.dataset.layout = layout;
      render();
    },
    play() {
      return destroyed ? Promise.reject(new Error("film is destroyed")) : controller.play();
    },
    pause() {
      controller.pause();
    },
    setMuted(muted) {
      if (!destroyed) controller.setMuted(muted);
    },
    setVolume(volume) {
      if (!destroyed) controller.setVolume(volume);
    },
    setBare(bare) {
      if (!destroyed) controller.setBare(bare);
    },
    subscribe(listener) {
      listeners.add(listener);
      listener(current);
      return () => listeners.delete(listener);
    },
    anchor(id) {
      const el = destroyed ? null : filmRoot.querySelector(`[data-film-anchor="${CSS.escape(id)}"]`);
      return el ? el.getBoundingClientRect() : null;
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      stopLoading();
      listeners.clear();
      reactRoot.unmount();
      leaveScope();
      host.remove();
    },
  };
}
