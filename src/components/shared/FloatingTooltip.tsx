import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

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
  const tipRef = useRef<HTMLDivElement>(null);
  const tooltipId = useId();
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  const updatePos = useCallback(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const width = Math.min(420, Math.max(220, window.innerWidth - 24));
    const measuredHeight = tipRef.current?.offsetHeight ?? 72;
    const gap = 8;
    const left = Math.max(12, Math.min(rect.left, window.innerWidth - width - 12));
    const top =
      rect.top > measuredHeight + gap + 12
        ? rect.top - measuredHeight - gap
        : rect.bottom + gap;
    setPos({ left, top });
  }, []);

  const show = useCallback(() => {
    if (disabled) return;
    setOpen(true);
    requestAnimationFrame(updatePos);
  }, [disabled, updatePos]);

  useEffect(() => {
    if (!open) return;
    updatePos();
    const onDismiss = (event: MouseEvent | PointerEvent) => {
      const target = event.target as Node;
      if (anchorRef.current?.contains(target) || tipRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("resize", updatePos);
    window.addEventListener("scroll", updatePos, true);
    document.addEventListener("pointerdown", onDismiss, true);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("resize", updatePos);
      window.removeEventListener("scroll", updatePos, true);
      document.removeEventListener("pointerdown", onDismiss, true);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, updatePos]);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  const label = typeof content === "string" ? content : undefined;
  const tipClassName = [
    "fixed z-50 max-h-[40vh] overflow-auto rounded-lg border px-3 py-2 text-[12px] leading-5 shadow-[0_14px_34px_rgba(0,0,0,0.42)] backdrop-blur-xl",
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
      {open &&
        pos &&
        createPortal(
          <div
            id={tooltipId}
            ref={tipRef}
            role="tooltip"
            className={tipClassName}
            style={{
              left: pos.left,
              top: pos.top,
              width: "min(26.25rem, calc(100vw - 1.5rem))",
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, black 12%)",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
              color: "var(--ct-fg)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              ...tooltipStyle,
            }}
          >
            {content}
          </div>,
          document.body,
        )}
    </>
  );
}
