import { useState, type CSSProperties, type ReactNode } from "react";
import { PortalHostContext } from "../../mocks/portalHost";

/** Product portals (modals, menus) land inside this box, so the camera frames and moves them with the shot. */
export function PortalStage({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  return <div style={{ position: "absolute", inset: 0, ...style }}>
    {host && <PortalHostContext.Provider value={host}>{children}</PortalHostContext.Provider>}
    <div ref={setHost} data-film-portal-host style={{ position: "absolute", inset: 0, pointerEvents: "none" }} />
  </div>;
}
