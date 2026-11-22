export const DICTATION_WAVEFORM_BARS = 5;

const NOISE_FLOOR = 0.006;
const SPEECH_CEILING = 0.22;
const ATTACK_ALPHA = 0.55;
const DECAY_ALPHA = 0.28;
const SILENCE_SNAP = 0.01;
const SILENT_THRESHOLD = 0.04;

const BAR_WEIGHTS = [0.55, 0.85, 1.0, 0.82, 0.52] as const;
const RESTING_BARS = [0.24, 0.3, 0.34, 0.28, 0.22] as const;

export interface DictationLevelSample {
  level: number;
  tick: number;
}

/** Maps raw PCM RMS (0..1) onto a speech-sensitive visual level (0..1). */
export function normalizeDictationRms(rms: number): number {
  if (!Number.isFinite(rms) || rms <= NOISE_FLOOR) return 0;
  return Math.min(1, Math.sqrt((rms - NOISE_FLOOR) / SPEECH_CEILING));
}

/** Smooths level transitions with a fast attack for syllables and gentle decay between words. */
export function smoothDictationLevel(previous: number, rawRms: number): number {
  const prev = Number.isFinite(previous) ? Math.max(0, Math.min(1, previous)) : 0;
  const target = normalizeDictationRms(rawRms);
  const alpha = target >= prev ? ATTACK_ALPHA : DECAY_ALPHA;
  const next = prev + (target - prev) * alpha;
  return next < SILENCE_SNAP ? 0 : Math.min(1, next);
}

/** Five compact bar heights (0.2..1) for the dictation meter beside "Listening". */
export function dictationWaveformBars(level: number, tick: number): number[] {
  if (!Number.isFinite(level) || level < SILENT_THRESHOLD) return [...RESTING_BARS];
  const clamped = Math.min(1, level);
  return BAR_WEIGHTS.map((weight, index) => {
    const wave = 0.84 + 0.16 * Math.sin(tick * 0.9 + index * 1.35);
    return Math.min(1, 0.22 + 0.78 * clamped * weight * wave);
  });
}

export function isDictationLevelSilent(level: number): boolean {
  return !Number.isFinite(level) || level < SILENT_THRESHOLD;
}

export const QUIET_DICTATION_SAMPLE: DictationLevelSample = Object.freeze({ level: 0, tick: 0 });

/** Leaf-scoped level bus so live meter updates never re-render the surrounding composer. */
export function createDictationLevelChannel() {
  let sample: DictationLevelSample = QUIET_DICTATION_SAMPLE;
  const listeners = new Set<() => void>();

  function emit(next: DictationLevelSample) {
    sample = next;
    for (const listener of listeners) listener();
  }

  return {
    read: (): DictationLevelSample => sample,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    pushRms(rawRms: number) {
      const next = smoothDictationLevel(sample.level, rawRms);
      if (next === 0 && sample.level === 0) return;
      emit({ level: next, tick: sample.tick + 1 });
    },
    reset() {
      if (sample === QUIET_DICTATION_SAMPLE) return;
      emit(QUIET_DICTATION_SAMPLE);
    },
  };
}
