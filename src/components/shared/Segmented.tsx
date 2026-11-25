import {
  useCallback,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
  title?: string;
  count?: ReactNode;
  disabled?: boolean;
}

interface SegmentedProps<T extends string> {
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  height?: number;
  iconOnly?: boolean;
  className?: string;
  "data-testid"?: string;
  "data-tab-id"?: string;
  "aria-label"?: string;
}

interface SegmentTravel {
  key: number;
  from: number;
  to: number;
}

function nextEnabledIndex<T extends string>(
  options: readonly SegmentedOption<T>[],
  from: number,
  step: 1 | -1,
): number {
  const count = options.length;
  for (let offset = 1; offset <= count; offset += 1) {
    const candidate = (from + step * offset + count) % count;
    if (!options[candidate].disabled) return candidate;
  }
  return from;
}

function boundaryEnabledIndex<T extends string>(
  options: readonly SegmentedOption<T>[],
  fromEnd: boolean,
): number {
  const count = options.length;
  for (let i = 0; i < count; i += 1) {
    const candidate = fromEnd ? count - 1 - i : i;
    if (!options[candidate].disabled) return candidate;
  }
  return -1;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  height = 24,
  iconOnly = false,
  className = "",
  ...rest
}: SegmentedProps<T>) {
  const nextKey = useRef(0);
  const [travel, setTravel] = useState<SegmentTravel | null>(null);
  const activeIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const select = useCallback(
    (index: number) => {
      if (options[index].disabled) return;
      if (index !== activeIndex) {
        nextKey.current += 1;
        setTravel({ key: nextKey.current, from: activeIndex, to: index });
      }
      onChange(options[index].value);
    },
    [activeIndex, onChange, options],
  );
  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      const key = event.code || event.key;
      let next: number;
      switch (key) {
        case "ArrowRight":
        case "ArrowDown":
          next = nextEnabledIndex(options, activeIndex, 1);
          break;
        case "ArrowLeft":
        case "ArrowUp":
          next = nextEnabledIndex(options, activeIndex, -1);
          break;
        case "Home":
          next = boundaryEnabledIndex(options, false);
          break;
        case "End":
          next = boundaryEnabledIndex(options, true);
          break;
        default:
          return;
      }
      if (next < 0 || next === activeIndex || options[next]?.disabled) return;
      event.preventDefault();
      event.stopPropagation();
      event.currentTarget.querySelectorAll<HTMLButtonElement>(".ct-liquid-segmented__option")[next]?.focus();
      select(next);
    },
    [activeIndex, options, select],
  );
  const style = {
    height,
    "--seg-height": `${height}px`,
    "--seg-count": options.length,
    "--seg-index": activeIndex,
  } as CSSProperties;
  const thumbStyle = travel
    ? {
        "--sx-from": `${travel.from * 100}%`,
        "--sx-to": `${travel.to * 100}%`,
      } as CSSProperties
    : undefined;
  const rootClassName = [
    "ct-liquid-segmented",
    iconOnly && "ct-liquid-segmented--icon-only",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      role="tablist"
      aria-label={rest["aria-label"]}
      data-testid={rest["data-testid"]}
      data-tab-id={rest["data-tab-id"]}
      className={rootClassName}
      style={style}
      onKeyDown={onKeyDown}
    >
      <span
        key={travel?.key ?? 0}
        aria-hidden
        className={`ct-liquid-segmented__thumb${travel ? " ct-liquid-segmented__thumb--travel" : ""}`}
        style={thumbStyle}
        onAnimationEnd={() => setTravel(null)}
      />
      {options.map((option, index) => (
        <button
          key={option.value}
          type="button"
          role="tab"
          className="ct-liquid-segmented__option"
          data-value={option.value}
          aria-label={iconOnly ? option.label : undefined}
          aria-selected={index === activeIndex}
          tabIndex={index === activeIndex ? 0 : -1}
          disabled={option.disabled}
          aria-disabled={option.disabled || undefined}
          title={option.title ?? option.label}
          onMouseDown={(event) => event.preventDefault()}
          onClick={(event) => {
            if (event.currentTarget.parentElement?.contains(document.activeElement)) {
              event.currentTarget.focus();
            }
            select(index);
          }}
        >
          {option.icon && <span className="ct-liquid-segmented__icon">{option.icon}</span>}
          {!iconOnly && <span>{option.label}</span>}
          {!iconOnly && option.count != null && <span className="ct-liquid-segmented__count">{option.count}</span>}
        </button>
      ))}
    </div>
  );
}
