import { useContext, type ReactNode } from "react";
import { createPortal as createDomPortal, flushSync as flushDomSync } from "react-dom";
import { PortalHostContext } from "./portalHost";

export const flushSync = flushDomSync;

function StagePortal({ children, container }: { children: ReactNode; container: Element | DocumentFragment }) {
  const host = useContext(PortalHostContext);
  return createDomPortal(children, host && container === document.body ? host : container);
}

export function createPortal(children: ReactNode, container: Element | DocumentFragment, key?: string | null) {
  return <StagePortal key={key ?? undefined} container={container}>{children}</StagePortal>;
}
