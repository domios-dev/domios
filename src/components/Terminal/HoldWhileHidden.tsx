import { memo, type ReactNode } from "react";

export const HoldWhileHidden = memo(function HoldWhileHidden({ visible, children }: { visible: boolean; children: ReactNode }) {
  return visible ? <>{children}</> : null;
});
