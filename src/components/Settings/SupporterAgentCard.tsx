import { useId } from "react";
import { ExternalLink, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { AboutLink, InlineLinkButton, SettingsCard } from "./shared";
import { useDecisionToken } from "./useDecisionToken";

export const DEFAULT_DECISION_ADAPTER = "typesafe-jev";
const DOCS_URL = "https://docs.typesafe.ai";

interface Props {
  adapter: string;
  apiBase: string;
  apiHeaders: HeadersInit;
  onOpenGuide: (section: HelpSectionId) => void;
}

export function SupporterAgentCard({ adapter, apiBase, apiHeaders, onOpenGuide }: Props) {
  const inputId = useId();
  const t = useDecisionToken(adapter, apiBase, apiHeaders);

  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Supporter Agent"
      description="A fast decision model that helps CodeTerm choose: it re-ranks memory recall and flags when nothing relevant was found. It only needs an API token."
    >
      <form onSubmit={t.saveToken} className="flex flex-col gap-2" data-testid="supporter-agent-token-form">
        <FormRow label="API token" htmlFor={inputId} hint="Write-only. It is sent to the local daemon and never shown again.">
          <input
            id={inputId}
            ref={t.tokenRef}
            type="password"
            autoComplete="off"
            spellCheck={false}
            defaultValue=""
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            data-testid="supporter-agent-token-input"
          />
        </FormRow>
        <div className="flex items-center gap-2">
          <Button type="submit" variant="accent" size="sm" disabled={t.busy} data-testid="supporter-agent-token-save">
            Save token
          </Button>
          <Button type="button" variant="ghost" size="sm" disabled={t.busy} onClick={() => void t.clearToken()} data-testid="supporter-agent-token-clear">
            Clear token
          </Button>
        </div>
      </form>

      <div className="flex flex-col gap-1 text-[11px] text-[var(--ct-muted-fg)]" role="status" data-testid="supporter-agent-status">
        {t.tokenState && (
          <p>
            Token for {adapter}: <span data-testid="supporter-agent-token-state">{t.tokenState}</span>
          </p>
        )}
        {t.statusFailed && <p data-testid="supporter-agent-status-failed">Token status is unavailable.</p>}
        {t.modeWarning && (
          <p data-testid="supporter-agent-mode-warning">Warning: the token file permissions are not owner-only.</p>
        )}
        {t.adapterMissing && (
          <p data-testid="supporter-agent-adapter-missing">The adapter {adapter} is not registered in this build.</p>
        )}
        {t.error && <p role="alert" data-testid="supporter-agent-error">{t.error}</p>}
      </div>

      <div className="flex flex-wrap items-center gap-4" data-testid="supporter-agent-help">
        <InlineLinkButton label="How it works" onClick={() => onOpenGuide("supporter-agent")} testId="supporter-agent-guide-link" />
        <AboutLink href={DOCS_URL} label="TypeSafe docs" Icon={ExternalLink} />
      </div>
    </SettingsCard>
  );
}
