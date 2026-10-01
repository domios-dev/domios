import { useEffect, useRef, type ReactNode } from "react";
import type { SettingsCardAnchorId } from "./sections";

/** A deep-link landing spot: when focused, the card scrolls into view and takes focus for keyboard users. */
export function SettingsCardAnchor({
  id,
  focused,
  children,
}: {
  id: SettingsCardAnchorId;
  focused: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!focused) return;
    ref.current?.scrollIntoView?.({ block: "start" });
    ref.current?.focus({ preventScroll: true });
  }, [focused]);

  return (
    <div
      ref={ref}
      tabIndex={-1}
      data-settings-card={id}
      data-settings-card-focused={focused || undefined}
      className="scroll-mt-4 rounded-xl outline-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]"
    >
      {children}
    </div>
  );
}
