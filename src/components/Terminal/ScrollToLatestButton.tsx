import { useRef, type MutableRefObject } from "react";
import { jumpTapAccepted } from "./chatJumpIntent";
import { ChevronDown } from "lucide-react";

interface Props {
  compact?: boolean;
  className?: string;
  testId: string;
  title?: string;
  onClick: () => void;
  gestureOwned?: boolean;
  userScrollingRef?: MutableRefObject<boolean>;
}

export function ScrollToLatestButton({
  compact = false,
  className = "",
  testId,
  title = "Scroll to latest",
  onClick,
  gestureOwned = false,
  userScrollingRef,
}: Props) {
  const beganIdle = useRef<boolean | null>(null);
  return (
    <button
      type="button"
      data-testid={testId}
      data-compact={compact || undefined}
      onPointerDown={() => { beganIdle.current = !(userScrollingRef?.current ?? gestureOwned); }}
      onPointerCancel={() => { beganIdle.current = null; }}
      onClick={(event) => {
        const allowed = !userScrollingRef || (!userScrollingRef.current
          && jumpTapAccepted((event.detail === 0 && beganIdle.current == null) || beganIdle.current === true));
        beganIdle.current = null;
        if (allowed) onClick();
      }}
      title={title}
      aria-label={title}
      className={`ct-scroll-latest ${gestureOwned ? "pointer-events-none" : "pointer-events-auto"} ${className}`}
    >
      <ChevronDown size={compact ? 13 : 16} strokeWidth={2.2} />
    </button>
  );
}
