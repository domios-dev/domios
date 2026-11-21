import { useEffect, useRef, useSyncExternalStore } from "react";
import { X } from "lucide-react";
import { COMPOSER_BUTTON_BASE } from "../composerButton";
import {
  dictationWaveformBars,
  isDictationLevelSilent,
  QUIET_DICTATION_SAMPLE,
} from "./dictationLevelModel";
import type { LiveDictation } from "./useLiveDictation";

const NOOP_SUBSCRIBE = () => () => {};
const READ_QUIET = () => QUIET_DICTATION_SAMPLE;

function DictationWaveform({ dictation, phase }: { dictation: LiveDictation; phase: "starting" | "listening" | "finishing" }) {
  const sample = useSyncExternalStore(
    dictation.subscribeLevel ?? NOOP_SUBSCRIBE,
    dictation.readLevel ?? READ_QUIET,
    dictation.readLevel ?? READ_QUIET,
  );
  const liveLevel = phase === "listening" ? sample.level : 0;
  const bars = dictationWaveformBars(liveLevel, sample.tick);
  const silent = phase !== "listening" || isDictationLevelSilent(liveLevel);
  return (
    <span
      className="ct-dictation-waveform"
      data-testid="dictation-waveform"
      data-phase={phase}
      data-level={liveLevel.toFixed(2)}
      data-silent={silent ? "true" : "false"}
      aria-hidden="true"
    >
      {bars.map((height, index) => (
        <span
          key={index}
          className="ct-dictation-bar"
          style={{ transform: `scaleY(${height.toFixed(3)})` }}
        />
      ))}
    </span>
  );
}

/** Quiet, live voice presence rendered inside the composer's trailing action slot without layout jump. */
export function DictationPresence({ dictation }: { dictation: LiveDictation }) {
  if (dictation.phase === "idle") return null;
  const phase = dictation.phase;
  const label = phase === "starting"
    ? "Starting dictation"
    : phase === "finishing"
      ? "Finishing dictation"
      : "Listening";
  return (
    <div className="ct-dictation-presence" data-testid="dictation-presence" data-phase={phase}>
      <span role="status" className="sr-only">{label}</span>
      <DictationWaveform dictation={dictation} phase={phase} />
      <button
        type="button"
        className={`${COMPOSER_BUTTON_BASE} ct-composer-inner-control ct-dictation-cancel rounded-full border-0 bg-transparent`}
        aria-label="Cancel dictation"
        title="Cancel dictation"
        onPointerDown={event => event.preventDefault()}
        onClick={dictation.cancel}
      >
        <X size={14} strokeWidth={2.2} aria-hidden="true" />
      </button>
    </div>
  );
}

/** Compact floating glass error bubble anchored to the round mic button. */
export function DictationStatus({ dictation }: { dictation: LiveDictation }) {
  const bubbleRef = useRef<HTMLDivElement>(null);
  const dismissRef = useRef(dictation.dismissError);
  dismissRef.current = dictation.dismissError;
  const hasError = Boolean(dictation.error);

  useEffect(() => {
    if (!hasError) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.code !== "Escape" && event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismissRef.current?.();
    };
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      const shell = bubbleRef.current?.closest(".ct-composer-action-shell");
      if (bubbleRef.current?.contains(target) || shell?.contains(target)) return;
      dismissRef.current?.();
    };
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [hasError]);

  if (!dictation.error) return null;
  return (
    <div ref={bubbleRef} className="ct-dictation-bubble ct-dictation-bubble--error" data-testid="dictation-error-bubble">
      <div className="ct-dictation-bubble__head">
        <span role="alert" className="ct-dictation-error">{dictation.error}</span>
        {dictation.dismissError && (
          <button
            type="button"
            className="ct-dictation-bubble__close"
            aria-label="Dismiss dictation error"
            title="Dismiss"
            onClick={dictation.dismissError}
          >
            <X size={14} strokeWidth={2.2} aria-hidden="true" />
          </button>
        )}
      </div>
    </div>
  );
}

/** Calm dot + word header status shown in the expanded composer's top bar while dictation is active. */
export function ExpandedDictationStatus({ phase }: { phase: LiveDictation["phase"] }) {
  if (phase === "idle") return null;
  const label = phase === "starting" ? "Starting" : phase === "finishing" ? "Finishing" : "Listening";
  return (
    <span className="ct-expanded-dictation-status" data-testid="expanded-dictation-status" data-phase={phase}>
      <span className="ct-expanded-dictation-dot" aria-hidden="true" />
      <span>{label}</span>
    </span>
  );
}
