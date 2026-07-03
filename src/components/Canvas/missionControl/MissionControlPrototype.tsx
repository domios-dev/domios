import { useMemo, useState } from "react";
import { MISSION_CONTROL_FIXTURE } from "./fixtures";
import type { MissionControlLod, MissionControlVariant } from "./types";
import { MISSION_CONTROL_VARIANTS, VARIANT_LABELS } from "./types";
import { VariantAssemblyLine } from "./variants/VariantAssemblyLine";
import { VariantControlRoom } from "./variants/VariantControlRoom";
import { VariantHangar } from "./variants/VariantHangar";

const VARIANT_COMPONENTS = {
  hangar: VariantHangar,
  "control-room": VariantControlRoom,
  "assembly-line": VariantAssemblyLine,
} as const;

export function MissionControlPrototype() {
  const [variant, setVariant] = useState<MissionControlVariant>("assembly-line");
  const [lod, setLod] = useState<MissionControlLod>("detail");
  const snapshot = MISSION_CONTROL_FIXTURE;
  const VariantView = useMemo(() => VARIANT_COMPONENTS[variant], [variant]);

  return (
    <div className="mc-root mc-gallery-shell" data-testid="mission-control-prototype">
      <header className="mc-gallery-toolbar">
        <div>
          <h1>Canvas mission control · concept gallery</h1>
          <div className="mc-sub">Plan operations room — user gate before live build</div>
        </div>
        <div className="mc-pill-row" role="tablist" aria-label="Layout variant">
          {MISSION_CONTROL_VARIANTS.map((v) => (
            <button
              key={v}
              type="button"
              className="mc-pill"
              role="tab"
              aria-selected={variant === v}
              data-active={variant === v}
              onClick={() => setVariant(v)}
            >
              {VARIANT_LABELS[v]}
            </button>
          ))}
        </div>
        <div className="mc-pill-row" role="tablist" aria-label="Level of detail">
          {(["detail", "overview"] as const).map((level) => (
            <button
              key={level}
              type="button"
              className="mc-pill"
              role="tab"
              aria-selected={lod === level}
              data-active={lod === level}
              onClick={() => setLod(level)}
            >
              {level === "detail" ? "100% LOD" : "Zoomed-out LOD"}
            </button>
          ))}
        </div>
      </header>
      <div className="mc-stage-wrap">
        <div className="mc-stage" data-lod={lod} data-variant={variant}>
          <VariantView snapshot={snapshot} />
        </div>
      </div>
    </div>
  );
}
