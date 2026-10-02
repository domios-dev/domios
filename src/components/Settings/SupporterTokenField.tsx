import { useState, type FormEvent, type RefObject } from "react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";

interface Props {
  mode: "required" | "optional";
  hasToken: boolean;
  busy: boolean;
  tokenRef: RefObject<HTMLInputElement | null>;
  saveToken: (event: FormEvent<HTMLFormElement>) => Promise<boolean>;
  clearToken: () => Promise<void>;
}

export function SupporterTokenField({ mode, hasToken, busy, tokenRef, saveToken, clearToken }: Props) {
  const [editing, setEditing] = useState(false);
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const showInput = !hasToken || editing;
  const label = `${hasToken ? "Replace token" : "API token"}${mode === "optional" ? " (optional)" : ""}`;

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    if (await saveToken(event)) setEditing(false);
  };

  return (
    <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-1" data-testid="supporter-agent-token-form">
      <label htmlFor="supporter-agent-token-input" className="text-[11px] text-[var(--ct-muted-fg)]">
        {label}
      </label>
      <div className="flex items-center gap-2">
        {showInput ? (
          <input
            id="supporter-agent-token-input"
            ref={tokenRef}
            type="password"
            autoComplete="off"
            spellCheck={false}
            defaultValue=""
            autoFocus={editing}
            aria-label={label}
            placeholder={mode === "optional" ? "Leave empty if the server needs no token" : "Paste the API token"}
            className={`${UI_INPUT.fullCompact} min-w-0 flex-1`}
            data-testid="supporter-agent-token-input"
          />
        ) : (
          <span
            className={`${UI_INPUT.fullCompact} min-w-0 flex-1 select-none tracking-[0.2em] text-[var(--ct-muted-fg)]`}
            aria-label="Saved token, hidden"
            data-testid="supporter-agent-token-masked"
          >
            ••••••••••••••••
          </span>
        )}
        {showInput ? (
          <>
            <Button type="submit" variant="accent" size="sm" disabled={busy} data-testid="supporter-agent-token-save">
              {hasToken ? "Replace token" : "Save token"}
            </Button>
            {editing && (
              <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setEditing(false)}>
                Cancel
              </Button>
            )}
          </>
        ) : (
          <>
            <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setEditing(true)} data-testid="supporter-agent-token-replace">
              Replace
            </Button>
            <Button type="button" variant="ghost" size="sm" disabled={busy || confirmingRemove} onClick={() => setConfirmingRemove(true)} data-testid="supporter-agent-token-clear">
              Remove
            </Button>
          </>
        )}
      </div>
      {showInput && (
        <span className="text-[10px] text-[var(--ct-muted)]">Stored by the local daemon and never shown again.</span>
      )}
      {confirmingRemove && !showInput && (
        <div
          role="alertdialog"
          aria-label="Remove the Supporter token"
          data-testid="supporter-agent-token-remove-confirm"
          className="flex flex-wrap items-center gap-2 rounded-md border border-[var(--ct-red)]/40 bg-[var(--ct-red)]/10 px-2 py-1.5 text-xs text-[var(--ct-fg)]"
        >
          <span className="flex-1">Remove the token? The Supporter stops until a new token is saved.</span>
          <Button
            type="button"
            variant="danger"
            size="sm"
            disabled={busy}
            autoFocus
            onClick={() => void clearToken().then(() => setConfirmingRemove(false))}
            data-testid="supporter-agent-token-remove-confirmed"
          >
            Remove token
          </Button>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setConfirmingRemove(false)}>
            Keep
          </Button>
        </div>
      )}
    </form>
  );
}
