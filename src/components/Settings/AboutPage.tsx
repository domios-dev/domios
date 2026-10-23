import { ExternalLink, Info, MessagesSquare, Scale, Users } from "lucide-react";
import { CommunityLinks } from "../shared/CommunityLinks";
import { LegalLink } from "../shared/LegalLinks";
import { useOptionalUpdatePanel } from "../../hooks/useUpdatePanel";
import { useUpdate } from "../../hooks/useUpdate";
import type { AppConfig } from "../../lib/types";
import { BrandMark } from "../brand/BrandMark";
import { DotMatrixCanvas } from "../brand/DotMatrixCanvas";
import { useBusyWaves } from "../brand/useBusyWaves";
import { usePrefersReducedMotion } from "../brand/usePrefersReducedMotion";
import { AboutLink, InfoRow, SettingsCard, SettingsPage } from "./shared";
import { UpdateSection } from "./UpdateSection";

const AUTHORS = [
  { name: "Andrey Kovalev", links: [
    { label: "GitHub", href: "https://github.com/rollacode" },
    { label: "Website", href: "https://rollacode.org" },
    { label: "LinkedIn", href: "https://www.linkedin.com/in/rollacode" },
  ] },
  { name: "Anna Ivanova", links: [
    { label: "LinkedIn", href: "https://www.linkedin.com/in/ainyiva/" },
  ] },
];

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

        <CommunityLinks icon={<MessagesSquare size={16} />} />

        <SettingsCard icon={<Users size={16} />} title="Authors">
          <div className="flex flex-col">
            {AUTHORS.map(author => (
              <div key={author.name} className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 border-b border-[var(--ct-border)] py-2 first:pt-0 last:border-0 last:pb-0">
                <span className="text-xs font-medium text-[var(--ct-fg)]">{author.name}</span>
                <div className="flex flex-wrap gap-3">
                  {author.links.map(link => <AboutLink key={link.label} href={link.href} label={link.label} Icon={ExternalLink} />)}
                </div>
              </div>
            ))}
          </div>
        </SettingsCard>

        <SettingsCard icon={<Scale size={16} />} title="Legal">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <LegalLink document="terms" />
            <LegalLink document="privacy" />
            <LegalLink document="license" />
          </div>
        </SettingsCard>
      </div>
    </SettingsPage>
  );
}
