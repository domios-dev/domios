import type { ReactNode } from "react";
import { sidebarDrawerLayout } from "./sidebarDrawerLayout";

interface SidebarDrawerProps {
  phone: boolean;
  shown: boolean;
  children: ReactNode;
}

export function SidebarDrawer({ phone, shown, children }: SidebarDrawerProps) {
  return (
    <div
      data-testid="sidebar-drawer"
      className={`ct-sidebar-drawer ${shown ? "is-open" : ""} flex min-h-0 overflow-visible`}
      style={sidebarDrawerLayout(phone)}
      aria-hidden={!shown}
      inert={!shown || undefined}
    >
      {children}
    </div>
  );
}
