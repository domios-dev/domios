import { memo, type ReactNode } from "react";

/** Hidden chats keep their last rendered subtree; parent renders reach it again once visible. */
export const HoldWhileHidden = memo(function HoldWhileHidden({ children }: { visible: boolean; children: ReactNode }) {
  return <>{children}</>;
}, (_previous, next) => !next.visible);
