import { useCallback, useRef, useState, type CSSProperties } from "react";

type SegmentedProps = {
  options: string[];
  value: number;
  onChange: (index: number) => void;
  ariaLabel: string;
};

// Segmented control. Travel is ONE liquid keyframe gesture (stretch in flight,
// splat on the wall, relax) driven by --sx-from/--sx-to; the key remount
export function LiquidSegmented({ options, value, onChange, ariaLabel }: SegmentedProps) {
  const [travelKey, setTravelKey] = useState(0);
  const from = useRef<number | null>(null);

  const select = useCallback(
    (index: number) => {
      if (index !== value) {
        from.current = value;
        setTravelKey((k) => k + 1);
      }
      onChange(index);
    },
    [value, onChange],
  );

  const style = {
    "--seg-count": options.length,
    "--seg-index": value,
  } as CSSProperties;

  const traveling = from.current !== null;
  const thumbStyle = traveling
    ? ({ "--sx-from": `${(from.current as number) * 100}%`, "--sx-to": `${value * 100}%` } as CSSProperties)
    : undefined;

  return (
    <div className="lg-seg" role="tablist" aria-label={ariaLabel} style={style}>
      <span
        key={travelKey}
        className={`lg-seg__thumb${traveling ? " lg-seg__thumb--travel" : ""}`}
        style={thumbStyle}
        aria-hidden
        onAnimationEnd={() => {
          from.current = null;
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

// Knob toggle: the knob crosses the track as a droplet — same liquid gesture,
// rem-based coordinates.
export function LiquidToggle({ checked, onChange, ariaLabel }: ToggleProps) {
  const [travelKey, setTravelKey] = useState(0);
  const traveling = useRef(false);

  const flip = useCallback(() => {
    traveling.current = true;
    setTravelKey((k) => k + 1);
    onChange(!checked);
  }, [checked, onChange]);

  // By render time `checked` is already the destination — from is its opposite.
  const knobStyle = traveling.current
    ? ({
        "--kx-from": checked ? "0rem" : "1.55rem",
        "--kx-to": checked ? "1.55rem" : "0rem",
      } as CSSProperties)
    : undefined;

  return (
    <button
      type="button"
      className="lg-toggle"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={flip}
    >
      <span
        key={travelKey}
        className={`lg-toggle__knob${traveling.current ? " lg-toggle__knob--travel" : ""}`}
        style={knobStyle}
        aria-hidden
        onAnimationEnd={() => {
          traveling.current = false;
        }}
      />
    </button>
  );
}

// Labelled flip toggle (off | on): the raised chip is the same liquid body,
// percent-based like the segmented thumb.
export function LiquidFlip({ checked, onChange, ariaLabel }: ToggleProps) {
  const [travelKey, setTravelKey] = useState(0);
  const traveling = useRef(false);

  const flip = useCallback(() => {
    traveling.current = true;
    setTravelKey((k) => k + 1);
    onChange(!checked);
  }, [checked, onChange]);

  // By render time `checked` is already the destination — from is its opposite.
  const chipStyle = traveling.current
    ? ({
        "--sx-from": checked ? "0%" : "100%",
        "--sx-to": checked ? "100%" : "0%",
      } as CSSProperties)
    : undefined;

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
        key={travelKey}
        className={`lg-flip__chip${traveling.current ? " lg-flip__chip--travel" : ""}`}
        style={chipStyle}
        aria-hidden
        onAnimationEnd={() => {
          traveling.current = false;
        }}
      />
      <span className="lg-flip__word" data-active={!checked}>off</span>
      <span className="lg-flip__word" data-active={checked}>on</span>
    </button>
  );
}
