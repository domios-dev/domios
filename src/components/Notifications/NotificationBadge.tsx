import { useEffect, useRef, useState } from "react";

interface NotificationBadgeProps {
  count: number;
}

export function NotificationBadge({ count }: NotificationBadgeProps) {
  const [pulse, setPulse] = useState(false);
  const prevCountRef = useRef(count);

  useEffect(() => {
    if (count > prevCountRef.current) {
      setPulse(true);
      const timer = setTimeout(() => setPulse(false), 1000);
      return () => clearTimeout(timer);
    }
    prevCountRef.current = count;
  }, [count]);

  if (count <= 0) return null;

  return (
    <span
      className={`
        inline-flex items-center justify-center min-w-[16px] h-4 px-1
        rounded-full bg-[var(--ct-red)] text-white text-[10px] font-bold leading-none
        ${pulse ? "animate-pulse" : ""}
      `}
    >
      {count > 99 ? "99+" : count}
    </span>
  );
}
