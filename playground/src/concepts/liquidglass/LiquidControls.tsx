import { useCallback, useRef, useState, type CSSProperties } from "react";

type SegmentedProps = {
  options: string[];
  value: number;
  onChange: (index: number) => void;
  ariaLabel: string;
};

// Segmented control: the thumb travels on a plain ease-out and bounces off the
// destination wall (directional squash, clipped by the track). The hit class
// re-arms per change, keyed off animationend — no timers.
export function LiquidSegmented({ options, value, onChange, ariaLabel }: SegmentedProps) {
  const [hitKey, setHitKey] = useState(0);
  const direction = useRef<"left" | "right" | null>(null);

  const select = useCallback(
    (index: number) => {
      if (index !== value) {
        direction.current = index > value ? "right" : "left";
        setHitKey((k) => k + 1);
      }
      onChange(index);
    },
    [value, onChange],
  );

  const style = {
    "--seg-count": options.length,
    "--seg-index": value,
  } as CSSProperties;

  const hitClass = direction.current ? ` lg-seg__thumb--hit-${direction.current}` : "";

  return (
    <div className="lg-seg" role="tablist" aria-label={ariaLabel} style={style}>
      <span
        key={hitKey}
        className={`lg-seg__thumb${hitClass}`}
        aria-hidden
        onAnimationEnd={() => {
          direction.current = null;
        }}
      />
      {options.map((opt, index) => (
        <button
          key={opt}
          type="button"
          role="tab"
          className="lg-seg__opt"
          aria-selected={index === value}
          onClick={() => select(index)}
        >
          {opt}
        </button>
      ))}
    </div>
  );
}

type ToggleProps = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  ariaLabel: string;
};

// Knob toggle: plain ease-out travel; the :active squash is the touch response.
export function LiquidToggle({ checked, onChange, ariaLabel }: ToggleProps) {
  return (
    <button
      type="button"
      className="lg-toggle"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={() => onChange(!checked)}
    >
      <span className="lg-toggle__knob" aria-hidden />
    </button>
  );
}

// Labelled flip toggle (off | on): the raised chip slides to the active word
// and bumps the pill wall on arrival.
export function LiquidFlip({ checked, onChange, ariaLabel }: ToggleProps) {
  const [hitKey, setHitKey] = useState(0);
  const direction = useRef<"left" | "right" | null>(null);

  const flip = useCallback(() => {
    direction.current = checked ? "left" : "right";
    setHitKey((k) => k + 1);
    onChange(!checked);
  }, [checked, onChange]);

  const hitClass = direction.current ? ` lg-flip__chip--hit-${direction.current}` : "";

  return (
    <button
      type="button"
      className="lg-flip"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={flip}
    >
      <span
        key={hitKey}
        className={`lg-flip__chip${hitClass}`}
        aria-hidden
        onAnimationEnd={() => {
          direction.current = null;
        }}
      />
      <span className="lg-flip__word" data-active={!checked}>off</span>
      <span className="lg-flip__word" data-active={checked}>on</span>
    </button>
  );
}
