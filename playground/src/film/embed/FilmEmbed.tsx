import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { ProductProviders } from "../../gallery/fixtures";
import type { Scene } from "../engine";
import { BreakoutProvider, spillReach, type Breakout } from "../breakout";
import { GuestProvider } from "../guest";
import { PortalStage } from "../fixtures/stage";
import { Captions, cardsOf, type Word } from "../player";
import { scenesLoading, type Cut } from "../reels/library";
import { Frame } from "./frame";
import { useHints } from "./hints";
import { STAGE_CONTAINER, STAGE_VH, STAGE_VW } from "./rootClass";
import { __film, setStage } from "./scope";
import { useScale, useWheelPassThrough } from "./stageHooks";
import type { FilmTheme } from "./theme";
import { chapterAt, chaptersOf, cuesAt, isWeakDevice, reachAt, MEMORY_GRAPH_CUTS, spanOf } from "./timeline";

export type FilmLayout = "landscape" | "portrait";

export type PlayResult = "sound";

export interface FilmState {
  started: boolean;
  playing: boolean;
  muted: boolean;
  ended: boolean;
  chapter: string | null;
  pausedBy: "viewer" | "host" | null;
  /** The first of `cues`, or null. */
  cue: string | null;
  /** Every named beat active right now, such as "burst", "memory", "jev", "night"; unknown ones are safe to ignore. */
  cues: string[];
}

/** A real link the end card offers, such as Download or GitHub; the page owns where it goes and what a click does. */
export interface FilmAction {
  id: string;
  label: string;
  /** A second, smaller line under the label, like the site's "x64 · Latest release". */
  detail?: string;
  href: string;
  primary?: boolean;
  ariaLabel?: string;
  onClick?: (event: ReactMouseEvent<HTMLAnchorElement>) => void;
}

export interface FilmController {
  pause: () => void;
  play: () => Promise<PlayResult>;
  setMuted: (muted: boolean) => void;
  setVolume: (volume: number) => void;
  setBare: (bare: boolean) => void;
  emit: (state: FilmState) => void;
  ready: () => void;
}

export interface FilmEmbedProps {
  scene: Scene;
  audioUrl: string;
  captionsUrl: string;
  theme: FilmTheme;
  layout: FilmLayout;
  reducedMotion: boolean;
  controller: FilmController;
  actions: FilmAction[];
  guest: string | null;
}

const STAGE: Record<FilmLayout, { width: number; height: number }> = {
  landscape: { width: 1920, height: 1080 },
  portrait: { width: 1080, height: 1920 },
};
const SEEK_STEP = 5;
const UNMUTE_VOLUME = 0.6;
const STILL_MS = 2500;
/** How far past the frame, in stage pixels, drawings may spill into the page around a landscape film. */
const BREAKOUT_MARGIN = { landscape: { x: 140, y: 200 }, portrait: { x: 40, y: 260 } };

const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

/** Fallback clips sit next to the audio file and are named after the reel, layout and theme. */
export const fallbackClipUrl = (audioUrl: string, reel: string, layout: FilmLayout, theme: FilmTheme) =>
  new URL(`${reel}.fallback-${layout}-${theme}.mp4`, new URL(audioUrl, document.baseURI)).href;

const Icon = ({ d, size = 18 }: { d: string; size?: number }) => (
  <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false"><path d={d} fill="currentColor" /></svg>
);
const PLAY = "M8 5.5v13a1 1 0 0 0 1.5.86l10.4-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5Z";
const PAUSE = "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z";
const CHAPTERS = "M4 6h2v2H4zm4 0h12v2H8zm-4 5h2v2H4zm4 0h12v2H8zm-4 5h2v2H4zm4 0h12v2H8z";
const VOLUME = "M4 9.5h3.2L12 5v14l-4.8-4.5H4zM15.5 8.5a5 5 0 0 1 0 7l-1.1-1.1a3.4 3.4 0 0 0 0-4.8zm2.4-2.4a8.4 8.4 0 0 1 0 11.8l-1.1-1.1a6.8 6.8 0 0 0 0-9.6z";
const MUTED = "M4 9.5h3.2L12 5v14l-4.8-4.5H4zM15.2 9.3l1.1-1.1 2.2 2.2 2.2-2.2 1.1 1.1-2.2 2.2 2.2 2.2-1.1 1.1-2.2-2.2-2.2 2.2-1.1-1.1 2.2-2.2z";
const REPLAY = "M12 5a7 7 0 1 1-6.6 4.7l1.5.5A5.4 5.4 0 1 0 12 6.6V9L8 5.8 12 2.6z";
const DOWNLOAD = "M11 4h2v8.6l3.3-3.3 1.4 1.4L12 16.4l-5.7-5.7 1.4-1.4 3.3 3.3zM5 18h14v2H5z";
const GITHUB = "M12 2a10 10 0 0 0-3.2 19.5c.5.1.7-.2.7-.5v-1.7c-2.8.6-3.4-1.3-3.4-1.3-.4-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.4 1.1 2.9.8.1-.6.4-1.1.6-1.3-2.2-.3-4.6-1.1-4.6-5 0-1.1.4-2 1-2.7-.1-.2-.4-1.3.1-2.7 0 0 .8-.3 2.8 1a9.6 9.6 0 0 1 5 0c1.9-1.3 2.8-1 2.8-1 .5 1.4.2 2.4.1 2.7.6.7 1 1.6 1 2.7 0 3.9-2.3 4.7-4.6 5 .4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5A10 10 0 0 0 12 2z";
const ARROW = "M13 5l7 7-7 7-1.4-1.4 4.6-4.6H4v-2h12.2l-4.6-4.6z";
const ACTION_ICONS: Record<string, string> = { download: DOWNLOAD, github: GITHUB };
const CC = "M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Zm2.5 4A1.5 1.5 0 0 0 6 10.5v3A1.5 1.5 0 0 0 7.5 15h2a1.5 1.5 0 0 0 1.5-1.5V13H9.5v.5h-2v-3h2v.5H11v-.5A1.5 1.5 0 0 0 9.5 9h-2Zm7 0a1.5 1.5 0 0 0-1.5 1.5v3a1.5 1.5 0 0 0 1.5 1.5h2a1.5 1.5 0 0 0 1.5-1.5V13h-1.5v.5h-2v-3h2v.5H18v-.5A1.5 1.5 0 0 0 16.5 9h-2Z";

export function FilmEmbed({ scene, audioUrl, captionsUrl, theme, layout, reducedMotion, controller, actions, guest }: FilmEmbedProps) {
  const { width, height } = STAGE[layout];
  setStage(width, height);
  const duration = scene.duration;
  const cuts = useMemo(() => (__film.globals.__filmCuts as Record<string, Cut[]> | undefined)?.[scene.id] ?? [], [scene.id]);
  const pulses = useMemo(() => cuts.slice(1).map((c) => c.start), [cuts]);
  const chapters = useMemo(() => chaptersOf(cuts, duration), [cuts, duration]);
  const memorySpan = useMemo(() => spanOf(cuts, MEMORY_GRAPH_CUTS), [cuts]);

  const [box, setBox] = useState<HTMLDivElement | null>(null);
  const scale = useScale(box, width);
  useWheelPassThrough(box);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [started, setStarted] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [t, setT] = useState(0);
  const [cards, setCards] = useState<Word[][]>([]);
  const [captionsOn, setCaptionsOn] = useState(true);
  const [weak, setWeak] = useState<boolean | null>(null);
  /** A phone-sized or touch-first device: the film there skips its effects past the frame. */
  const compact = useMemo(() => layout === "portrait" || (typeof window !== "undefined" && !!window.matchMedia?.("(pointer: coarse)").matches), [layout]);
  const [clipReady, setClipReady] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [pointerInside, setPointerInside] = useState(false);
  const [keyboardInside, setKeyboardInside] = useState(false);
  const [touch, setTouch] = useState(false);
  // Bare: the host shows the film edge to edge with no controls; a tap still pauses it.
  const [bare, setBare] = useState(false);
  useEffect(() => {
    controller.setBare = setBare;
  }, [controller]);
  const [still, setStill] = useState(false);
  const [moves, setMoves] = useState(0);
  const [muted, setMuted] = useState(false);
  const [volume, setVolume] = useState(1);
  const [ended, setEnded] = useState(false);
  const [pausedBy, setPausedBy] = useState<FilmState["pausedBy"]>(null);
  const pauseCause = useRef<"viewer" | "host">("viewer");
  const [stageEl, setStageEl] = useState<HTMLDivElement | null>(null);

  useEffect(() => () => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
    audioRef.current = null;
  }, []);

  const ensureAudio = () => {
    if (audioRef.current) return audioRef.current;
    const audio = new Audio();
    audio.preload = "auto";
    audio.src = audioUrl;
    const sync = (e: Event) => {
      setPlaying(!audio.paused);
      setT(audio.currentTime);
      setEnded(audio.ended);
      setMuted(audio.muted);
      setVolume(audio.volume);
      if (e.type === "play" || audio.ended) setPausedBy(null);
      else if (e.type === "pause") setPausedBy(pauseCause.current);
      if (e.type === "pause") pauseCause.current = "viewer";
    };
    for (const type of ["play", "pause", "ended", "seeked", "volumechange"]) audio.addEventListener(type, sync);
    audioRef.current = audio;
    void fetch(captionsUrl).then((r) => r.json()).then((words: Word[]) => setCards(cardsOf(words)), () => setCards([]));
    return audio;
  };

  const seek = (to: number) => {
    const audio = ensureAudio();
    audio.currentTime = Math.max(0, Math.min(duration, to));
    setT(audio.currentTime);
    setEnded(false);
  };

  const begin = (focus: boolean) => {
    const audio = ensureAudio();
    if (!started) {
      setStarted(true);
      setT(audio.currentTime);
      if (focus) box?.focus({ preventScroll: true });
    }
    if (audio.ended) seek(0);
    return audio;
  };
  const play = () => void begin(true).play().catch(() => setPlaying(false));
  const pause = (cause: "viewer" | "host") => {
    const audio = audioRef.current;
    if (!audio || audio.paused) return;
    pauseCause.current = cause;
    audio.pause();
  };
  const toggle = () => (playing ? pause("viewer") : play());
  const toggleMute = () => {
    const audio = ensureAudio();
    if (audio.muted && audio.volume === 0) audio.volume = UNMUTE_VOLUME;
    audio.muted = !audio.muted;
  };
  const changeVolume = (next: number) => {
    const audio = ensureAudio();
    audio.volume = next;
    audio.muted = next === 0;
  };

  // The page scrolls the film into view: no focus move, and the sound stays as it is. A browser that refuses sound
  // without a gesture rejects the start, and the film stays unstarted rather than muted.
  const hostPlay = async (): Promise<PlayResult> => {
    const audio = ensureAudio();
    if (audio.ended) seek(0);
    await audio.play();
    begin(false);
    return "sound";
  };

  useEffect(() => {
    controller.pause = () => pause("host");
    controller.play = hostPlay;
    controller.setMuted = (next) => {
      ensureAudio().muted = next;
    };
    controller.setVolume = (next) => {
      ensureAudio().volume = Math.max(0, Math.min(1, next));
    };
    controller.ready();
  });

  useEffect(() => {
    const audio = audioRef.current;
    if (!playing || !audio) return;
    let raf = 0;
    let last = 0;
    const intervals: number[] = [];
    const tick = (now: number) => {
      // Frames slowed by scene chunks still downloading and evaluating say nothing about the device; they aren't counted.
      if (last && weak === null && !scenesLoading()) {
        intervals.push(now - last);
        const verdict = isWeakDevice(intervals);
        if (verdict !== null) setWeak(verdict);
      }
      last = now;
      setT(audio.currentTime);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, weak]);

  // Like a video player: while playing, the controls go when the mouse leaves or rests; touch, keyboard focus or a pause keep them.
  useEffect(() => {
    setStill(false);
    if (!playing) return;
    const timer = setTimeout(() => setStill(true), STILL_MS);
    return () => clearTimeout(timer);
  }, [playing, moves]);
  const chromeHidden = (bare && (playing || ended)) || (playing && !menuOpen && (!pointerInside || still) && !keyboardInside && !touch);

  const frameT = Math.min(t, duration - 1e-3);
  const inMemory = !!memorySpan && frameT >= memorySpan.start && frameT < memorySpan.end;
  const useClip = weak === true && !!memorySpan;
  const showClip = useClip && clipReady && inMemory && started;

  useEffect(() => {
    const video = videoRef.current;
    if (!video || !memorySpan) return;
    const target = frameT - memorySpan.start;
    if (!inMemory) {
      if (!video.paused) video.pause();
      return;
    }
    if (Math.abs(video.currentTime - target) > 0.25) video.currentTime = target;
    if (playing && video.paused) void video.play().catch(() => undefined);
    if (!playing && !video.paused) video.pause();
  });

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.target instanceof HTMLInputElement && e.code !== "Space") return;
    if (e.target instanceof Node && stageEl?.contains(e.target)) return;
    if (e.code === "Space" || e.code === "KeyK") toggle();
    else if (e.code === "ArrowRight") seek(t + SEEK_STEP);
    else if (e.code === "ArrowLeft") seek(t - SEEK_STEP);
    else if (e.code === "Escape" && menuOpen) setMenuOpen(false);
    else return;
    e.preventDefault();
  };

  const chapter = chapterAt(chapters, t);
  const chapterTitle = started ? chapter?.title ?? null : null;
  const cueKey = started && !ended ? cuesAt(cuts, frameT).join(" ") : "";
  useEffect(() => {
    const cues = cueKey ? cueKey.split(" ") : [];
    controller.emit({ started, playing, muted, ended, chapter: chapterTitle, pausedBy, cue: cues[0] ?? null, cues });
  }, [controller, started, playing, muted, ended, chapterTitle, pausedBy, cueKey]);

  const exploring = started && ended;
  const hints = useHints(stageEl, started && !ended);
  const [breakoutEl, setBreakoutEl] = useState<HTMLDivElement | null>(null);
  // Like every effect outside the frame, the spill runs only while the film plays; a pause gives the page back.
  // Never on a phone: the spill canvases (a frame plus its reach on every side) are the largest buffers the film draws,
  // and with the live scene's own they can cost a phone tab its GPU memory and get it reloaded.
  const spills = started && playing && !reducedMotion && weak !== true && !exploring && !compact;
  // Over the scenic span the page goes dark and the memory spreads far past the frame, then draws back in.
  const reach = Math.round(reachAt(cuts, frameT) * 100) / 100;
  const margin = BREAKOUT_MARGIN[layout];
  const far = spillReach(layout === "portrait");
  const mx = Math.round(margin.x + (far.x - margin.x) * reach);
  const my = Math.round(margin.y + (far.y - margin.y) * reach);
  const breakout = useMemo<Breakout | null>(() => (spills && breakoutEl ? { node: breakoutEl, margin: { x: mx, y: my } } : null), [spills, breakoutEl, mx, my]);
  let stage: ReactNode = <Frame scene={scene} t={frameT} width={width} height={height} pulses={pulses} scale={scale} />;
  if (showClip) stage = <div style={{ width, height, background: "var(--ct-bg)" }} />;

  return (
    <ProductProviders>
      <div
        ref={setBox}
        className="film-box"
        data-started={started || undefined}
        data-chrome-hidden={chromeHidden || undefined}
        data-reduced-motion={reducedMotion || undefined}
        data-fallback={weak === null ? undefined : weak ? "clip" : "live"}
        data-ended={exploring || undefined}
        role="region"
        aria-label="Domios film"
        tabIndex={0}
        onKeyDown={onKeyDown}
        onPointerEnter={(e) => e.pointerType !== "touch" && setPointerInside(true)}
        onPointerMove={(e) => e.pointerType !== "touch" && setMoves((n) => n + 1)}
        onPointerLeave={(e) => e.pointerType !== "touch" && setPointerInside(false)}
        onPointerDown={(e) => setTouch(e.pointerType === "touch")}
        onFocus={(e) => setKeyboardInside(e.target.matches(":focus-visible"))}
        onBlur={(e) => { if (!(e.relatedTarget instanceof Node && e.currentTarget.contains(e.relatedTarget))) setKeyboardInside(false); }}
      >
        <div ref={setStageEl} className="film-stage" onClick={exploring ? undefined : started ? toggle : play}>
          {scale > 0 && (
            <div
              className="film-stage-inner"
              style={{ width, height, transform: `scale(${scale})`, containerName: STAGE_CONTAINER, [STAGE_VW as string]: `${width / 100}px`, [STAGE_VH as string]: `${height / 100}px` }}
            >
              <div inert aria-hidden="true" style={{ position: "absolute", inset: 0 }}>
                <GuestProvider value={guest}>
                  <BreakoutProvider value={breakout}>
                    <PortalStage key={layout}>{stage}</PortalStage>
                  </BreakoutProvider>
                </GuestProvider>
              </div>
              {useClip && (
                <video
                  ref={videoRef}
                  className="film-clip"
                  src={fallbackClipUrl(audioUrl, scene.id, layout, theme)}
                  muted
                  playsInline
                  preload="auto"
                  onLoadStart={() => setClipReady(false)}
                  onLoadedData={() => setClipReady(true)}
                  style={{ width, height, visibility: showClip ? "visible" : "hidden" }}
                />
              )}
              {started && !exploring && captionsOn && <Captions cards={cards} t={t} width={width} height={height} />}
            </div>
          )}
        </div>
        <div ref={setBreakoutEl} className="film-breakout" aria-hidden="true" style={{ width, height, transform: `scale(${scale})` }} />

        {!started && (
          <button type="button" className="film-poster-play" onClick={play} aria-label={`Play the film, ${clock(duration)}`}>
            <span className="film-poster-disc"><Icon d={PLAY} size={34} /></span>
            <span className="film-poster-time">{clock(duration)}</span>
          </button>
        )}

        <div ref={hints.ring} className="film-hint-ring" aria-hidden="true" />
        <div ref={hints.chip} className="film-hint" aria-hidden="true"><b /><span /></div>

        {exploring && (
          <div className="film-end" onClick={(e) => e.stopPropagation()}>
            {actions.length > 0 && (
              <div className="film-end-actions">
                {actions.map((action, index) => (
                  <a
                    key={action.id}
                    className="film-end-action"
                    data-primary={action.primary || undefined}
                    href={action.href}
                    aria-label={action.ariaLabel}
                    style={{ animationDelay: `${0.35 + index * 0.12}s` }}
                    onClick={action.onClick}
                    {...(action.primary ? {} : { target: "_blank", rel: "noopener" })}
                  >
                    {!action.primary && <Icon d={ACTION_ICONS[action.id] ?? ARROW} size={18} />}
                    <span className="film-end-label">
                      <span>{action.label}</span>
                      {action.detail && <small>{action.detail}</small>}
                    </span>
                    <Icon d={action.primary ? ACTION_ICONS[action.id] ?? ARROW : ARROW} size={action.primary ? 18 : 16} />
                  </a>
                ))}
              </div>
            )}
            <button type="button" className="film-replay" onClick={() => { seek(0); play(); }}>
              <Icon d={REPLAY} size={16} />
              <span>Replay</span>
            </button>
          </div>
        )}

        {started && !exploring && (
          <div className="film-bar" onClick={(e) => e.stopPropagation()}>
            <button type="button" className="film-button" onClick={toggle} aria-label={playing ? "Pause" : "Play"}>
              <Icon d={playing ? PAUSE : PLAY} />
            </button>
            <div className="film-volume">
              <button type="button" className="film-button" onClick={toggleMute} aria-label={muted ? "Unmute" : "Mute"}>
                <Icon d={muted || volume === 0 ? MUTED : VOLUME} />
              </button>
              <input
                type="range"
                className="film-volume-range"
                aria-label="Volume"
                min={0}
                max={1}
                step={0.05}
                value={muted ? 0 : volume}
                onChange={(e) => changeVolume(Number(e.target.value))}
                style={{ ["--film-progress" as string]: `${(muted ? 0 : volume) * 100}%` }}
              />
            </div>
            <span className="film-time">{clock(t)} / {clock(duration)}</span>
            <div className="film-track">
              <input
                type="range"
                aria-label="Seek"
                aria-valuetext={`${clock(t)}${chapter ? `, ${chapter.title}` : ""}`}
                min={0}
                max={duration}
                step={0.1}
                value={t}
                onChange={(e) => seek(Number(e.target.value))}
                style={{ ["--film-progress" as string]: `${(t / duration) * 100}%` }}
              />
              {chapters.slice(1).map((c) => (
                <span key={c.title} className="film-tick" style={{ left: `${(c.start / duration) * 100}%` }} />
              ))}
            </div>
            <button type="button" className="film-button film-chapter-button" onClick={() => setMenuOpen(!menuOpen)} aria-expanded={menuOpen} aria-label="Chapters">
              <Icon d={CHAPTERS} />
            </button>
            <button type="button" className="film-button" onClick={() => setCaptionsOn(!captionsOn)} aria-pressed={captionsOn} aria-label="Captions">
              <Icon d={CC} />
            </button>
            {menuOpen && (
              <ul className="film-menu" aria-label="Chapters">
                {chapters.map((c) => (
                  <li key={c.title}>
                    <button
                      type="button"
                      aria-current={c === chapter || undefined}
                      onClick={() => {
                        seek(c.start + 0.01);
                        setMenuOpen(false);
                        play();
                      }}
                    >
                      <span>{c.title}</span>
                      <span className="film-menu-time">{clock(c.start)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
    </ProductProviders>
  );
}
