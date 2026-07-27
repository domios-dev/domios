import { useCallback, useRef, useState, type CSSProperties } from "react";

interface LiquidTabRailProps<T extends string> {
  value: T;
  options: readonly T[];
  onChange: (value: T) => void;
  label: string;
  touch?: boolean;
  testIdPrefix?: string;
}

interface RailTravel {
  key: number;
  from: number;
  to: number;
}

export function LiquidTabRail<T extends string>({
  value,
  options,
  onChange,
  label,
  touch = false,
  testIdPrefix,
}: LiquidTabRailProps<T>) {
  const nextKey = useRef(0);
  const [travel, setTravel] = useState<RailTravel | null>(null);
  const activeIndex = Math.max(0, options.indexOf(value));
  const select = useCallback(
    (index: number) => {
      if (index !== activeIndex) {
        nextKey.current += 1;
        setTravel({ key: nextKey.current, from: activeIndex, to: index });
      }
      onChange(options[index]);
    },
    [activeIndex, onChange, options],
  );
  const style = {
    "--rail-count": options.length,
    "--rail-index": activeIndex,
  } as CSSProperties;
  const indicatorStyle = travel
    ? {
        "--sx-from": `${travel.from * 100}%`,
        "--sx-to": `${travel.to * 100}%`,
      } as CSSProperties
    : undefined;

  return (
    <div
      role="tablist"
      aria-label={label}
      className="ct-liquid-tab-rail"
      style={style}
    >
      <span
        key={travel?.key ?? 0}
        aria-hidden
        className={`ct-liquid-tab-rail__indicator${travel ? " ct-liquid-tab-rail__indicator--travel" : ""}`}
        style={indicatorStyle}
        onAnimationEnd={() => setTravel(null)}
      />
      {options.map((option, index) => (
        <button
          key={option}
          type="button"
          role="tab"
          aria-selected={index === activeIndex}
          data-testid={testIdPrefix ? `${testIdPrefix}-${option}` : undefined}
          className={`ct-liquid-tab-rail__tab${touch ? " ct-liquid-tab-rail__tab--touch" : ""}`}
          onClick={() => select(index)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}
