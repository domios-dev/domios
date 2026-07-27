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

function useLiquidTravel(checked: boolean, onCheckedChange: (checked: boolean) => void) {
  const [travelKey, setTravelKey] = useState(0);
  const traveling = useRef(false);
  const flip = useCallback(() => {
    traveling.current = true;
    setTravelKey((key) => key + 1);
    onCheckedChange(!checked);
  }, [checked, onCheckedChange]);
  return { flip, traveling, travelKey };
}

export function LiquidToggle({
  checked,
  onCheckedChange,
  className,
  ...props
}: LiquidToggleProps) {
  const { flip, traveling, travelKey } = useLiquidTravel(checked, onCheckedChange);
  const knobStyle = traveling.current
    ? {
        "--kx-from": checked ? "0rem" : "1.55rem",
        "--kx-to": checked ? "1.55rem" : "0rem",
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
        key={travelKey}
        aria-hidden
        className={`ct-liquid-toggle__knob${traveling.current ? " ct-liquid-toggle__knob--travel" : ""}`}
        style={knobStyle}
        onAnimationEnd={() => {
          traveling.current = false;
        }}
      />
    </button>
  );
}

export function LiquidFlip(props: LiquidToggleProps) {
  const { checked, onCheckedChange, className, ...buttonProps } = props;
  const { flip, traveling, travelKey } = useLiquidTravel(checked, onCheckedChange);
  const chipStyle = traveling.current
    ? {
        "--sx-from": checked ? "0%" : "100%",
        "--sx-to": checked ? "100%" : "0%",
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
        key={travelKey}
        aria-hidden
        className={`ct-liquid-flip__chip${traveling.current ? " ct-liquid-flip__chip--travel" : ""}`}
        style={chipStyle}
        onAnimationEnd={() => {
          traveling.current = false;
        }}
      />
      <span className="ct-liquid-flip__word" data-active={!checked}>off</span>
      <span className="ct-liquid-flip__word" data-active={checked}>on</span>
    </button>
  );
}
