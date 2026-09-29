import { useCallback, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";

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
  const moveFocus = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      const current = options.indexOf(value);
      let next: number;
      switch (event.key) {
        case "ArrowRight":
          next = (current + 1) % options.length;
          break;
        case "ArrowLeft":
          next = (current - 1 + options.length) % options.length;
          break;
        case "Home":
          next = 0;
          break;
        case "End":
          next = options.length - 1;
          break;
        default:
          return;
      }
      event.preventDefault();
      select(next);
      event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
    },
    [options, select, value],
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
      aria-orientation="horizontal"
      className="ct-liquid-tab-rail"
      style={style}
      onKeyDown={moveFocus}
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
          tabIndex={index === activeIndex ? 0 : -1}
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
