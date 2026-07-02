import { useEffect, useRef, useState } from "react";
import type { PaneStatus } from "../../lib/types";

export type TabStatusRailVariant = "working" | "attention" | "dead" | "settle";

export function resolveTabStatusRailVariant(
  status: PaneStatus | null | undefined,
  attention: boolean,
  settling: boolean,
): TabStatusRailVariant | null {
  if (settling) return "settle";
  if (status === "Dead") return "dead";
  if (status === "Waiting" || attention) return "attention";
  if (!status || status === "Idle") return null;
  if (status === "Working") return "working";
  return null;
}

export function TabStatusRail({
  status,
  attention = false,
}: {
  status: PaneStatus | null | undefined;
  attention?: boolean;
}) {
  const [settling, setSettling] = useState(false);
  const prevStatusRef = useRef<PaneStatus | null | undefined>(undefined);
  const settleGenRef = useRef(0);

  useEffect(() => {
    const prev = prevStatusRef.current;
    if (prev !== undefined && prev === "Working" && (status === "Idle" || status == null)) {
      const gen = ++settleGenRef.current;
      setSettling(true);
      const timer = globalThis.setTimeout(() => {
        if (settleGenRef.current === gen) setSettling(false);
      }, 900);
      prevStatusRef.current = status ?? null;
      return () => {
        globalThis.clearTimeout(timer);
      };
    }
    prevStatusRef.current = status ?? null;
  }, [status]);

  const variant = resolveTabStatusRailVariant(status, attention, settling);
  if (!variant) return null;

  return (
    <span
      aria-hidden="true"
      data-testid="tab-status-rail"
      data-rail-variant={variant}
      className={`ct-tab-status-rail ct-tab-status-rail--${variant}`}
    />
  );
}
