import { useCallback, useEffect, useState } from "react";
import { invoke, IS_WEB, listen } from "../lib/bridge";

type ProviderUpdateStatus =
  | "awaiting_consent"
  | "declined"
  | "deferred"
  | "installing"
  | "restarted"
  | "failed";

export interface ProviderUpdatePrompt {
  prompt_id: string;
  tab_id: string;
  provider_name: string;
  restart_required: boolean;
  status: ProviderUpdateStatus;
  message: string;
}

function upsert(
  prompts: ProviderUpdatePrompt[],
  next: ProviderUpdatePrompt,
): ProviderUpdatePrompt[] {
  return [next, ...prompts.filter((prompt) => prompt.prompt_id !== next.prompt_id)];
}

export function ProviderUpdatePromptLayer() {
  const [prompts, setPrompts] = useState<ProviderUpdatePrompt[]>([]);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (IS_WEB) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void invoke<ProviderUpdatePrompt[]>("list_provider_update_prompts")
      .then((pending) => {
        if (active) setPrompts(pending);
      })
      .catch((error) => console.warn("Unable to load provider update prompts", error));
    void listen<ProviderUpdatePrompt>("provider-update-prompt", ({ payload }) => {
      if (active) setPrompts((current) => upsert(current, payload));
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const prompt = prompts[0];
  const dismiss = useCallback(() => {
    if (!prompt) return;
    setPrompts((current) => current.filter((item) => item.prompt_id !== prompt.prompt_id));
  }, [prompt]);
  const respond = useCallback(
    async (accept: boolean) => {
      if (!prompt) return;
      setSubmitting(true);
      try {
        await invoke("respond_provider_update_prompt", {
          promptId: prompt.prompt_id,
          accept,
        });
        if (!accept) dismiss();
      } catch (error) {
        setPrompts((current) =>
          upsert(current, {
            ...prompt,
            status: "failed",
            message: String(error),
          }),
        );
      } finally {
        setSubmitting(false);
      }
    },
    [dismiss, prompt],
  );

  if (!prompt || IS_WEB) return null;
  const terminal = prompt.status === "declined" || prompt.status === "restarted" || prompt.status === "failed";
  const canChoose = prompt.status === "awaiting_consent" || prompt.status === "deferred";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="provider-update-title"
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/55 p-4"
    >
      <div className="w-full max-w-[460px] rounded-xl border border-[var(--ct-border-default)] bg-[var(--ct-bg-elev)] p-5 text-[var(--ct-fg)] shadow-2xl">
        <h2 id="provider-update-title" className="text-base font-semibold">
          {prompt.provider_name} update
        </h2>
        <p className="mt-2 text-sm leading-6 text-[var(--ct-muted)]">{prompt.message}</p>
        {prompt.restart_required && prompt.status === "awaiting_consent" && (
          <p className="mt-2 text-xs leading-5 text-[var(--ct-muted)]">
            Installing will restart this provider session after CodeTerm verifies that no delivery or turn is in flight.
          </p>
        )}
        <div className="mt-5 flex justify-end gap-2">
          {terminal && (
            <button type="button" onClick={dismiss} className="rounded border border-[var(--ct-border-default)] px-3 py-1.5 text-sm">
              Dismiss
            </button>
          )}
          {canChoose && (
            <>
              <button
                type="button"
                disabled={submitting}
                onClick={() => void respond(false)}
                className="rounded border border-[var(--ct-border-default)] px-3 py-1.5 text-sm disabled:opacity-50"
              >
                Decline
              </button>
              {prompt.status === "awaiting_consent" && (
                <button
                  type="button"
                  disabled={submitting}
                  onClick={() => void respond(true)}
                  className="rounded bg-[var(--ct-accent)] px-3 py-1.5 text-sm text-white disabled:opacity-50"
                >
                  Install update
                </button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
