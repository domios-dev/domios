import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { sidebarDrawerLayout } from "./sidebarDrawerLayout";

interface SidebarDrawerProps {
  phone: boolean;
  shown: boolean;
  children: ReactNode;
}

export function SidebarDrawer({ phone, shown, children }: SidebarDrawerProps) {
  // The drawer slides only when it opens or closes. Crossing into phone width (a narrowed window, a rotated tablet)
  // places a closed drawer off screen at once, instead of sliding it away over the content.
  const [sliding, setSliding] = useState(false);
  const previous = useRef(shown);
  useLayoutEffect(() => {
    if (previous.current === shown) return;
    previous.current = shown;
    setSliding(true);
  }, [shown]);
  return (
    <div
      data-testid="sidebar-drawer"
      className={`ct-sidebar-drawer ${shown ? "is-open" : ""} flex min-h-0 overflow-visible`}
      data-sliding={sliding || undefined}
      style={sidebarDrawerLayout(phone)}
      aria-hidden={!shown}
      inert={!shown || undefined}
      onTransitionEnd={(event) => { if (event.target === event.currentTarget && event.propertyName === "transform") setSliding(false); }}
    >
      {children}
    </div>
  );
}
