import { useCallback, useRef, useState, type CSSProperties } from "react";

type SegmentedProps = {
  options: string[];
  value: number;
  onChange: (index: number) => void;
  ariaLabel: string;
};

// Segmented control with a glass thumb that slides + stretches between equal
// segments. The morph class re-arms on every change so the squash-stretch
// replays each time (keyed off animationend, no timers).
export function LiquidSegmented({ options, value, onChange, ariaLabel }: SegmentedProps) {
  const [morphKey, setMorphKey] = useState(0);
  const morphing = useRef(false);

  const select = useCallback(
    (index: number) => {
      if (index !== value) {
        morphing.current = true;
        setMorphKey((k) => k + 1);
      }
      onChange(index);
    },
    [value, onChange],
  );

  const style = {
    "--seg-count": options.length,
    "--seg-index": value,
  } as CSSProperties;

  return (
    <div className="lg-seg" role="tablist" aria-label={ariaLabel} style={style}>
      <span
        key={morphKey}
        className={`lg-seg__thumb${morphing.current ? " lg-seg__thumb--morph" : ""}`}
        aria-hidden
        onAnimationEnd={() => {
          morphing.current = false;
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

// On/off toggle with a liquid knob that squashes as it travels and pops on
// arrival (CSS :active drives the squash keyframe).
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
