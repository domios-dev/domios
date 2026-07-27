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

interface LiquidTravel {
  key: number;
  from: boolean;
  to: boolean;
}

function useLiquidTravel(checked: boolean, onCheckedChange: (checked: boolean) => void) {
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

export function LiquidToggle({
  checked,
  onCheckedChange,
  className,
  ...props
}: LiquidToggleProps) {
  const { flip, travel, finishTravel } = useLiquidTravel(checked, onCheckedChange);
  const knobStyle = travel
    ? {
        "--kx-from": travel.from ? "1.55rem" : "0rem",
        "--kx-to": travel.to ? "1.55rem" : "0rem",
      } as CSSProperties
    : undefined;
  return (
    <button
      {...props}
      type="button"
      role="switch"
      aria-checked={checked}
      className={`ct-liquid-toggle ${className ?? ""}`}
      onClick={flip}
    >
      <span
        key={travel?.key ?? 0}
        aria-hidden
        className={`ct-liquid-toggle__knob${travel ? " ct-liquid-toggle__knob--travel" : ""}`}
        style={knobStyle}
        onAnimationEnd={finishTravel}
      />
    </button>
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
