import { ExternalLink, Info } from "lucide-react";
import { useOptionalUpdatePanel } from "../../hooks/useUpdatePanel";
import { useUpdate } from "../../hooks/useUpdate";
import type { AppConfig } from "../../lib/types";
import { BrandMark } from "../brand/BrandMark";
import { DotMatrixCanvas } from "../brand/DotMatrixCanvas";
import { useBusyWaves } from "../brand/useBusyWaves";
import { usePrefersReducedMotion } from "../brand/usePrefersReducedMotion";
import { AboutLink, InfoRow, SettingsCard, SettingsPage } from "./shared";
import { UpdateSection } from "./UpdateSection";

export function AboutPage({ config, onUpdate }: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const unified = useOptionalUpdatePanel();
  const app = useUpdate();
  const busy = unified ? unified.busy : app.installing;
  const failed = Boolean(unified ? unified.error : app.error);
  const reducedMotion = usePrefersReducedMotion();
  const { matrix, emitWave } = useBusyWaves(busy, failed);
  return (
    <SettingsPage title="Check for updates">
      <div className="brand-dot-stage flex flex-col gap-3">
        <DotMatrixCanvas ref={matrix} mode="loading" reducedMotion={reducedMotion} intensity={0.35} />
        <div className="flex flex-col items-center py-4 text-center">
          <span role="img" aria-label="Domios" data-testid="settings-about-logo" className="mb-3">
            <BrandMark motion={busy ? "load" : "static"} size={64} onRelease={emitWave} />
          </span>
          <div className="text-xl font-bold text-[var(--ct-fg)]">Domios</div>
          <div className="text-xs text-[var(--ct-muted-fg)] mt-1">AI-first terminal multiplexer</div>
        </div>

        <SettingsCard icon={<Info size={16} />} title="Info">
          <InfoRow label="Version" value={__APP_VERSION__} />
        </SettingsCard>

        <UpdateSection config={config} onUpdate={onUpdate} />

        <SettingsCard icon={<ExternalLink size={16} />} title="Created by">
          <div className="text-xs text-[var(--ct-fg)]">Andrey Kovalev</div>
          <div className="flex flex-wrap gap-3">
            <AboutLink href="https://github.com/rollacode" label="GitHub" Icon={ExternalLink} />
            <AboutLink href="https://rollacode.org" label="Website" Icon={ExternalLink} />
            <AboutLink href="https://www.linkedin.com/in/rollacode" label="LinkedIn" Icon={ExternalLink} />
          </div>
        </SettingsCard>
      </div>
    </SettingsPage>
  );
}
