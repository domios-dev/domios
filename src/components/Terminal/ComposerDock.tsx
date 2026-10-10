import type { ReactNode, Ref } from "react";
import type { ComposerMode } from "./composerLayoutModel";

export function ComposerDock({ mode, children, ref }: { mode: ComposerMode; children: ReactNode; ref?: Ref<HTMLDivElement> }) {
  return <div ref={ref} data-testid="chat-composer-shell" data-composer-mode={mode} className="ct-composer-dock relative shrink-0">
    <div className="ct-composer-lane relative mx-auto w-full min-w-0">{children}</div>
  </div>;
}
