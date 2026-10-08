import type { CapsuleModel } from "./surfaceChromeModel";

export function SurfaceCapsule({ capsule }: { capsule: CapsuleModel }) {
  return (
    <div role="status" aria-live="polite" data-testid="surface-capsule" data-tone={capsule.tone} className="ct-surface-capsule">
      <span aria-hidden="true" className="ct-surface-capsule__dot" />
      <span className="ct-surface-capsule__text">{capsule.text}</span>
    </div>
  );
}
