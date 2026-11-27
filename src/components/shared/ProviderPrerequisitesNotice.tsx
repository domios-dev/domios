import { CodeBlock } from "@codeterm/chat-ui/CodeBlock";
import { nativeBridge } from "../../lib/nativeBridge";
import { prerequisiteGuidance } from "../../lib/providerCli";
import { Button } from "./Button";

export interface PrerequisiteItem {
  binary: string;
  hint: string;
}

export function ProviderPrerequisitesNotice({
  prerequisites,
  onRetry,
  retryLabel = "Retry",
  compact = false,
}: {
  prerequisites: readonly PrerequisiteItem[];
  onRetry?: () => void;
  retryLabel?: string;
  compact?: boolean;
}) {
  if (prerequisites.length === 0) return null;
  const textClass = compact
    ? "m-0 break-words text-[11px] text-[var(--ct-muted-fg)]"
    : "m-0 break-words text-xs text-[var(--ct-muted-fg)]";

  return (
    <div
      data-testid="provider-prerequisites-notice"
      className="flex w-full min-w-0 flex-col gap-2 rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] p-2.5 text-left"
    >
      {prerequisites.map((item) => {
        const guidance = prerequisiteGuidance(item);
        return (
          <div key={item.binary} className="flex min-w-0 flex-col gap-1.5">
            <p role="status" className={textClass}>
              Needs {item.binary} first. {item.hint}
            </p>
            {guidance.remediationCommand && (
              <div aria-label={`Setup command for ${item.binary}`} role="region" className="w-full min-w-0">
                <CodeBlock
                  text={guidance.remediationCommand}
                  language={guidance.remediationLanguage}
                  wrapLines
                />
              </div>
            )}
            {guidance.installUrl && (
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  size="compact"
                  variant="ghost"
                  data-testid={`prerequisite-install-link-${item.binary}`}
                  aria-label={`Install ${item.binary} from ${guidance.installHostPath}`}
                  onClick={() => {
                    void nativeBridge.openUrl(guidance.installUrl!);
                  }}
                >
                  Install {item.binary} ({guidance.installHostPath})
                </Button>
              </div>
            )}
          </div>
        );
      })}
      {onRetry && (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="compact"
            variant="ghost"
            data-testid="prerequisite-retry-action"
            onClick={onRetry}
          >
            {retryLabel}
          </Button>
        </div>
      )}
    </div>
  );
}
