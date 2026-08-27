import {
  useCallback,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type CSSProperties,
} from "react";

export interface LiquidToggleProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onChange"> {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

export interface LiquidTravel {
  key: number;
  from: boolean;
  to: boolean;
}

/** One control at two sizes. Compact is the same switch at roughly half the
 *  area, for a host that puts the switch beside the words it governs. */
const KNOB_TRAVEL = {
  standard: { off: "0rem", on: "1.55rem" },
  compact: { off: "0px", on: "16px" },
} as const;

export type LiquidToggleSize = keyof typeof KNOB_TRAVEL;

export function useLiquidTravel(checked: boolean, onCheckedChange: (checked: boolean) => void) {
  const nextKey = useRef(0);
  const [travel, setTravel] = useState<LiquidTravel | null>(null);
  const flip = useCallback(() => {
    const nextChecked = !checked;
    nextKey.current += 1;
    setTravel({ key: nextKey.current, from: checked, to: nextChecked });
    onCheckedChange(nextChecked);
  }, [checked, onCheckedChange]);
  return { flip, travel, finishTravel: () => setTravel(null) };
}

function LiquidKnob({
  travel,
  size,
  onTravelEnd,
}: {
  travel: LiquidTravel | null;
  size: LiquidToggleSize;
  onTravelEnd: () => void;
}) {
  const distance = KNOB_TRAVEL[size];
  const style = travel
    ? ({
        "--kx-from": travel.from ? distance.on : distance.off,
        "--kx-to": travel.to ? distance.on : distance.off,
      } as CSSProperties)
    : undefined;
  return (
    <span
      aria-hidden
      className={`ct-liquid-toggle__knob${travel ? " ct-liquid-toggle__knob--travel" : ""}`}
      style={style}
      onAnimationEnd={onTravelEnd}
    />
  );
}

export function LiquidToggle({
  checked,
  onCheckedChange,
  className,
  ...props
}: LiquidToggleProps) {
  const { flip, travel, finishTravel } = useLiquidTravel(checked, onCheckedChange);
  return (
    <button
      {...props}
      type="button"
      role="switch"
      aria-checked={checked}
      className={`ct-liquid-toggle ${className ?? ""}`}
      onClick={flip}
    >
      <LiquidKnob
        key={travel?.key ?? 0}
        travel={travel}
        size="standard"
        onTravelEnd={finishTravel}
      />
    </button>
  );
}

/**
 * The same control drawn compact and decorative, for a host that is itself the
 * switch. It carries no role and no name: the accessible switch is the host, so
 * a reader hears the host's label once rather than meeting two controls.
 */
export function LiquidToggleTrack({
  checked,
  travel,
  onTravelEnd,
}: {
  checked: boolean;
  travel: LiquidTravel | null;
  onTravelEnd: () => void;
}) {
  return (
    <span aria-hidden className="ct-liquid-toggle ct-liquid-toggle--compact" data-checked={checked}>
      <LiquidKnob key={travel?.key ?? 0} travel={travel} size="compact" onTravelEnd={onTravelEnd} />
    </span>
  );
}

export function LiquidFlip(props: LiquidToggleProps) {
  const { checked, onCheckedChange, className, ...buttonProps } = props;
  const { flip, travel, finishTravel } = useLiquidTravel(checked, onCheckedChange);
  const chipStyle = travel
    ? {
        "--sx-from": travel.from ? "100%" : "0%",
        "--sx-to": travel.to ? "100%" : "0%",
      } as CSSProperties
    : undefined;
  return (
    <button
      {...buttonProps}
      type="button"
      role="switch"
      aria-checked={checked}
      className={`ct-liquid-flip ${className ?? ""}`}
      onClick={flip}
    >
      <span
        key={travel?.key ?? 0}
        aria-hidden
        className={`ct-liquid-flip__chip${travel ? " ct-liquid-flip__chip--travel" : ""}`}
        style={chipStyle}
        onAnimationEnd={finishTravel}
      />
      <span className="ct-liquid-flip__word" data-active={!checked}>off</span>
      <span className="ct-liquid-flip__word" data-active={checked}>on</span>
    </button>
  );
}
