import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { AnchoredPopover } from "./AnchoredPopover";

interface FloatingTooltipProps {
  content: ReactNode;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  tooltipClassName?: string;
  tooltipStyle?: CSSProperties;
  touchMode?: boolean;
  disabled?: boolean;
}

export function FloatingTooltip({
  content,
  children,
  className,
  style,
  tooltipClassName,
  tooltipStyle,
  touchMode = false,
  disabled = false,
}: FloatingTooltipProps) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const tooltipId = useId();
  const [open, setOpen] = useState(false);

  const show = useCallback(() => {
    if (!disabled) setOpen(true);
  }, [disabled]);
  const hide = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  const label = typeof content === "string" ? content : undefined;
  const tipClassName = [
    "max-h-[40vh] overflow-auto rounded-lg border px-3 py-2 text-[12px] leading-5 shadow-[0_14px_34px_rgba(0,0,0,0.42)] backdrop-blur-xl",
    tooltipClassName,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      <span
        ref={anchorRef}
        tabIndex={disabled ? undefined : 0}
        className={className}
        style={style}
        onPointerEnter={(event) => {
          if (event.pointerType === "mouse") show();
        }}
        onPointerLeave={(event) => {
          if (event.pointerType === "mouse") setOpen(false);
        }}
        onFocus={show}
        onBlur={() => setOpen(false)}
        onClick={(event) => {
          if (!touchMode || disabled) return;
          event.stopPropagation();
          if (open) {
            setOpen(false);
          } else {
            show();
          }
        }}
        aria-label={label}
        aria-describedby={open ? tooltipId : undefined}
      >
        {children}
      </span>
      <AnchoredPopover open={open} anchor={anchorRef} placement="top-start" gap={8} onClose={hide}>
          <div
            id={tooltipId}
            role="tooltip"
            className={tipClassName}
            style={{
              width: "26.25rem",
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 12%)",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
              color: "var(--ct-fg)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              ...tooltipStyle,
            }}
          >
            {content}
          </div>
      </AnchoredPopover>
    </>
  );
}
