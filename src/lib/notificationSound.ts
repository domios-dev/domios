/**
 * The completion sound.
 *
 * Sound is not a destination: it holds no record, cannot be read later and
 * cannot be answered, so it is presentation on a bell arrival alongside
 * `show_bell`, never a fourth column of the levels grid. That is also what
 * stops it disagreeing with routing — the daemon emits `notification-created`
 * inside the bell gate's admitted arm and nowhere else, so a level routed away
 * from the bell produces no arrival, and no arrival is no sound.
 */
import type { NotifyLevelName } from "./notifyLevels";

/**
 * Whether a level says the agent has stopped. "Finished" is not a level on the
 * shipped axis, so the sound keys on the two levels that already mean the agent
 * is no longer working and wants the user. Exhaustive over the level union: a
 * level added later cannot compile until it answers here.
 */
const AGENT_STOPPED: Record<NotifyLevelName, boolean> = {
  verbose: false,
  action_needed: true,
  failure: true,
};

/** An arrival naming no level claims nothing about the agent, so it stays silent. */
function announcesAgentStopped(level: string | null | undefined): boolean {
  if (!level) return false;
  return AGENT_STOPPED[level as NotifyLevelName] === true;
}

/**
 * The whole decision, as one predicate: an opted-in user, and an arrival that
 * says the agent stopped. There is no level configuration of its own to drift
 * from the grid — the arrival itself is the routing decision, already made.
 */
export function shouldSoundArrival(
  enabled: boolean | undefined,
  level: string | null | undefined,
): boolean {
  return enabled === true && announcesAgentStopped(level);
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
  createOscillator(): ChimeOscillator;
  createGain(): ChimeGain;
  resume?(): unknown;
}

const NOTE_HZ = [660, 880];
const NOTE_SECONDS = 0.09;
const PEAK_GAIN = 0.09;
const FLOOR_GAIN = 0.0001;

/**
 * A synthesised two-note blip. Nothing is scheduled here: the whole chime is
 * emitted on this call and the offsets below are the audio clock shaping one
 * sound, never control flow waiting on elapsed time.
 */
export function playArrivalChime(context: ChimeContext): void {
  context.resume?.();
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
