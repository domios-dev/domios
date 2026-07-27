import {
  useCallback,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
  title?: string;
}

interface SegmentedProps<T extends string> {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  height?: number;
  className?: string;
  "data-testid"?: string;
  "data-tab-id"?: string;
  "aria-label"?: string;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  height = 24,
  className = "",
  ...rest
}: SegmentedProps<T>) {
  const [travelKey, setTravelKey] = useState(0);
  const from = useRef<number | null>(null);
  const activeIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const select = useCallback(
    (index: number) => {
      if (index !== activeIndex) {
        from.current = activeIndex;
        setTravelKey((key) => key + 1);
      }
      onChange(options[index].value);
    },
    [activeIndex, onChange, options],
  );
  const traveling = from.current !== null;
  const style = {
    height,
    "--seg-count": options.length,
    "--seg-index": activeIndex,
  } as CSSProperties;
  const thumbStyle = traveling
    ? {
        "--sx-from": `${from.current! * 100}%`,
        "--sx-to": `${activeIndex * 100}%`,
      } as CSSProperties
    : undefined;

  return (
    <div
      role="tablist"
      aria-label={rest["aria-label"]}
      data-testid={rest["data-testid"]}
      data-tab-id={rest["data-tab-id"]}
      className={`ct-liquid-segmented ${className}`}
      style={style}
    >
      <span
        key={travelKey}
        aria-hidden
        className={`ct-liquid-segmented__thumb${traveling ? " ct-liquid-segmented__thumb--travel" : ""}`}
        style={thumbStyle}
        onAnimationEnd={() => {
          from.current = null;
        }}
      />
      {options.map((option, index) => (
        <button
          key={option.value}
          type="button"
          role="tab"
          className="ct-liquid-segmented__option"
          data-value={option.value}
          aria-selected={index === activeIndex}
          title={option.title ?? option.label}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => select(index)}
        >
          {option.icon && <span className="ct-liquid-segmented__icon">{option.icon}</span>}
          <span>{option.label}</span>
        </button>
      ))}
    </div>
  );
}
