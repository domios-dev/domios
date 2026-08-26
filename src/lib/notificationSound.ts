/**
 * The completion sound.
 *
 * Sound used to be deliberately kept OUT of the levels grid. The argument was
 * concrete: a column would let a user write `verbose: {bell: false, sound: true}`
 * — a level routed to reach nothing that still makes a noise, where the noise is
 * its only trace. That argument is OBSOLETE. The bell gate has been retired and
 * EVERY level now reaches the panel unconditionally, because the panel is the
 * complete log. No configuration can make a level sound while reaching nothing,
 * so every chime has a record behind it and a per-level chime is coherent.
 *
 * What each level does is therefore the level's own stored answer. The old
 * hardcoded table survives only as the answer an UNANSWERED row falls back to,
 * and any stated answer beats it in both directions.
 */
import type { AgentLevelChoice } from "./types";
import type { NotifyLevelName } from "./notifyLevels";

/**
 * The chime a level makes before anyone answers for it. Exhaustive over the
 * level union: a level added later cannot compile until it answers here. This
 * is a DEFAULT and never an override — a row that states an answer never
 * reaches it.
 */
const SHIPPED_SOUND: Record<NotifyLevelName, boolean> = {
  verbose: false,
  action_needed: true,
  failure: true,
};

export function shippedSound(level: string | null | undefined): boolean {
  if (!level) return false;
  return SHIPPED_SOUND[level as NotifyLevelName] === true;
}

/** This level's own answer, or the shipped one while it has none. */
export function levelSounds(
  choice: Pick<AgentLevelChoice, "level" | "sound"> | undefined,
  level: string | null | undefined,
): boolean {
  const answer: boolean | undefined = choice?.sound;
  return answer === undefined ? shippedSound(level) : answer;
}

/** The row governing one level, if the daemon has shipped one for it yet. */
export function levelChoice(
  levels: readonly AgentLevelChoice[],
  level: string | null | undefined,
): AgentLevelChoice | undefined {
  if (!level) return undefined;
  return levels.find((choice) => choice.level === level);
}

/** The caption of one level's chime control, and the tail of its accessible name. */
export const SOUND_CHOICE_LABEL = "Sound";

/** Move one level's chime answer. Writes the named field and nothing else, so a
 *  chime can never be mistaken for a push channel of the same name. */
export function setLevelSound(
  current: readonly AgentLevelChoice[],
  level: string,
  sound: boolean,
): AgentLevelChoice[] {
  return current.map((choice) => (choice.level === level ? { ...choice, sound } : choice));
}

/**
 * The whole decision: the master mute is on, and the arriving level's own row
 * says it chimes. An arrival naming no level has no row and therefore no answer,
 * so it stays silent rather than borrowing one.
 */
export function shouldSoundArrival(
  enabled: boolean | undefined,
  levels: readonly AgentLevelChoice[],
  level: string | null | undefined,
): boolean {
  if (enabled !== true || !level) return false;
  return levelSounds(levelChoice(levels, level), level);
}

interface ChimeParam {
  setValueAtTime(value: number, when: number): void;
  exponentialRampToValueAtTime(value: number, when: number): void;
}

interface ChimeGain {
  readonly gain: ChimeParam;
  connect(target: unknown): void;
}

interface ChimeOscillator {
  readonly frequency: ChimeParam;
  connect(target: ChimeGain): void;
  start(when: number): void;
  stop(when: number): void;
}

/** The slice of `AudioContext` a two-note chime needs, so a test can stand in for one. */
export interface ChimeContext {
  readonly currentTime: number;
  readonly destination: unknown;
  readonly state?: string;
  createOscillator(): ChimeOscillator;
  createGain(): ChimeGain;
  resume?(): unknown;
}

/**
 * A context born outside a user gesture starts suspended, and a suspended
 * context is silence. Resuming is therefore a state transition to make, not a
 * ritual to perform on every note.
 */
function resumeIfSuspended(context: ChimeContext): void {
  if (context.state === "suspended") context.resume?.();
}

const NOTE_HZ = [660, 880];
const NOTE_SECONDS = 0.09;
const PEAK_GAIN = 0.24;
const FLOOR_GAIN = 0.0001;

/**
 * A synthesised two-note blip. Nothing is scheduled here: the whole chime is
 * emitted on this call and the offsets below are the audio clock shaping one
 * sound, never control flow waiting on elapsed time.
 */
export function playArrivalChime(context: ChimeContext): void {
  resumeIfSuspended(context);
  const start = context.currentTime;
  NOTE_HZ.forEach((hz, index) => {
    const at = start + index * NOTE_SECONDS;
    const until = at + NOTE_SECONDS;
    const gain = context.createGain();
    gain.gain.setValueAtTime(PEAK_GAIN, at);
    gain.gain.exponentialRampToValueAtTime(FLOOR_GAIN, until);
    gain.connect(context.destination);
    const oscillator = context.createOscillator();
    oscillator.frequency.setValueAtTime(hz, at);
    oscillator.connect(gain);
    oscillator.start(at);
    oscillator.stop(until);
  });
}

type AudioContextCtor = new () => ChimeContext;

/** No asset and no pipeline: the chime is generated, so there is nothing to ship. */
export function openChimeContext(): ChimeContext | null {
  const host = globalThis as unknown as {
    AudioContext?: AudioContextCtor;
    webkitAudioContext?: AudioContextCtor;
  };
  const Ctor = host.AudioContext ?? host.webkitAudioContext;
  if (!Ctor) return null;
  try {
    return new Ctor();
  } catch {
    return null;
  }
}

let established: ChimeContext | null = null;

/**
 * Establish the chime's audio context. Call this from inside the gesture that
 * enables the sound: that is the only moment the browser lets a context start
 * running, so enabling the chime and hearing the very next arrival needs no
 * second click.
 */
export function establishChimeContext(): ChimeContext | null {
  established = established ?? openChimeContext();
  if (established) resumeIfSuspended(established);
  return established;
}

/** The context the enabling gesture established, if it got one. */
export function chimeContext(): ChimeContext | null {
  return established;
}

/**
 * Whether we hold a context that can actually be heard. A suspended context is
 * silence, so holding one is not the same as being established — the difference
 * is what keeps the gesture listener armed until a gesture really lands.
 */
export function chimeIsAudible(): boolean {
  return established !== null && established.state !== "suspended";
}

/** The events a browser accepts as the user gesture that may start audio. */
export const CHIME_GESTURE_EVENTS = ["pointerdown", "keydown"] as const;

export interface GestureTarget {
  addEventListener(type: string, handler: () => void): void;
  removeEventListener(type: string, handler: () => void): void;
}

/**
 * Establish the chime from the first user gesture anywhere in the app.
 *
 * The enabling click is a gesture, but a stored opt-in never passes through it:
 * on every restart the sound is already on and nothing has been clicked, so a
 * context opened later — inside a daemon event — starts suspended and stays
 * silent. The listener closes that gap and then removes itself, because there
 * is exactly one context to establish and nothing to keep watching for.
 *
 * It removes itself on an AUDIBLE context, not merely on having tried: a
 * gesture that failed to produce a running context has established nothing, and
 * giving up on it would leave the chime permanently silent.
 */
export function establishChimeOnFirstGesture(target: GestureTarget): () => void {
  let listening = true;
  const stop = () => {
    if (!listening) return;
    listening = false;
    for (const event of CHIME_GESTURE_EVENTS) target.removeEventListener(event, onGesture);
  };
  function onGesture(): void {
    establishChimeContext();
    if (chimeIsAudible()) stop();
  }
  for (const event of CHIME_GESTURE_EVENTS) target.addEventListener(event, onGesture);
  return stop;
}

export function __resetChimeContextForTest(): void {
  established = null;
}
