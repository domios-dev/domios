import { useEffect, useId, useRef, useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { FormDialog } from "../shared/FormDialog";
import { UI_BUTTON } from "../shared/design";
import { OrchestrationRoleRow } from "../Sidebar/OrchestrationPairForm";
import { ComposerTextInput } from "../Terminal/ComposerTextInput";
import { ComposerInputShell } from "../Terminal/ComposerInputShell";
import { ComposerActionButton } from "../Terminal/ComposerActionButton";
import { useComposerVoice } from "../Terminal/voice/useComposerVoice";
import { composerAttachmentHandoff } from "../Terminal/composerAttachmentHandoff";
import {
  CARTOGRAPHER_FOCUS_SOURCES,
  defaultCartographerMapDraft,
  defaultCartographerProvider,
  toggleCartographerFocusSource,
  type CartographerMapDraft,
  type CartographerMapMode,
} from "./cartographerMapModel";

export interface CartographerMapDialogProps {
  mode: CartographerMapMode;
  providers: ProviderDescriptor[];
  loading?: boolean;
  catalogueError?: string | null;
  onCancel: () => void;
  onStart: (draft: CartographerMapDraft) => Promise<void>;
  onAskOrchestrator?: () => Promise<void>;
}

function useCartographerMapForm({ mode, providers, onStart, onCancel }: CartographerMapDialogProps) {
  const [draft, setDraft] = useState(() => defaultCartographerMapDraft(mode, providers));
  const [submitting, setSubmitting] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    setDraft((current) => {
      if (providers.some((provider) => provider.id === current.providerId && isProviderSelectable(provider))) return current;
      const choice = defaultCartographerProvider(providers);
      return { ...current, providerId: choice.providerId, modelId: choice.modelId };
    });
  }, [providers]);
  const transcribe = async (paths: string[]) => {
    if (inFlight.current || paths.length === 0) return;
    setTranscribing(true);
    setError(null);
    try {
      const text = (await Promise.all(paths.map((path) => composerAttachmentHandoff.transcribeLocal(path)))).join("\n");
      if (mounted.current) setDraft((current) => ({ ...current, ownerDescription: current.ownerDescription ? `${current.ownerDescription}\n${text}` : text }));
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (mounted.current) setTranscribing(false);
    }
  };
  const submit = async () => {
    if (inFlight.current || transcribing || !draft.providerId) return;
    inFlight.current = true;
    setSubmitting(true);
    setError(null);
    try {
      await onStart(draft);
      onCancel();
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      inFlight.current = false;
      if (mounted.current) setSubmitting(false);
    }
  };
  return { draft, setDraft, submitting, transcribing, error, setError, transcribe, submit };
}

export function CartographerMapDialog(props: CartographerMapDialogProps) {
  const form = useCartographerMapForm(props);
  const titleId = useId();
  const descriptionId = useId();
  const inputId = useId();
  const voiceUi = useComposerVoice({
    hasText: false,
    onSend: () => {},
    onAttach: (paths) => { void form.transcribe(paths); },
    onSendWithAttachments: (paths) => { void form.transcribe(paths); },
    onError: form.setError,
  });
  const busy = form.submitting || form.transcribing || voiceUi.voiceBusy || voiceUi.voiceActive;
  const provider = props.providers.find((candidate) => candidate.id === form.draft.providerId);
  const unavailable = new Set(props.providers.filter((candidate) => !isProviderSelectable(candidate)).map((candidate) => candidate.id));
  const canStart = !props.loading && provider !== undefined && isProviderSelectable(provider) && !busy;
  return <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-lg"
    onCancel={() => { if (!form.submitting) props.onCancel(); }}
    onSubmit={(event) => { event.preventDefault(); if (canStart) void form.submit(); }}>
    <h2 id={titleId} className="text-sm font-semibold">{props.mode === "create" ? "Create map" : "Refresh map"}</h2>
    <p id={descriptionId} className="text-xs text-[var(--ct-muted-fg)]">
      A Cartographer will join this workspace’s Team and {props.mode === "create" ? "build the project essence map" : "update the map from recent changes"}.
      If the Team already has a Cartographer, it will receive this request.
    </p>
    <fieldset disabled={form.submitting} className="flex flex-col gap-2 text-xs">
      <legend className="mb-2 font-medium">Focus sources</legend>
      {CARTOGRAPHER_FOCUS_SOURCES.map((source) => <label key={source.id} className="flex items-center gap-2">
        <input type="checkbox" checked={form.draft.focusSources.includes(source.id)}
          onChange={() => form.setDraft((current) => ({ ...current, focusSources: toggleCartographerFocusSource(current.focusSources, source.id) }))} />
        {source.label}
      </label>)}
    </fieldset>
    {props.loading && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">Loading providers and models…</p>}
    {props.catalogueError && <p role="alert" className="text-xs text-[var(--ct-red)]">{props.catalogueError}</p>}
    <fieldset disabled={form.submitting}>
      <OrchestrationRoleRow label="Cartographer" providers={props.providers} providerId={form.draft.providerId}
        modelId={form.draft.modelId ?? ""} disabledProviderIds={unavailable}
        onProviderChange={(id) => {
          const choice = defaultCartographerProvider(props.providers.filter((candidate) => candidate.id === id));
          form.setDraft((current) => ({ ...current, providerId: choice.providerId, modelId: choice.modelId }));
        }}
        onModelChange={(id) => form.setDraft((current) => ({ ...current, modelId: id || undefined }))} />
    </fieldset>
    <p className="text-xs text-[var(--ct-muted-fg)]">You can also ask the Orchestrator at any time.</p>
    {props.onAskOrchestrator && <button type="button" className={UI_BUTTON.ghost} disabled={busy}
      onClick={() => { void props.onAskOrchestrator?.().then(props.onCancel, (reason: unknown) => form.setError(reason instanceof Error ? reason.message : String(reason))); }}>Ask the Orchestrator</button>}
    <label htmlFor={inputId} className="text-xs font-medium">What is this project about? (optional)</label>
    <div className="flex items-end gap-2">
      <ComposerInputShell voiceUi={voiceUi}>
        <ComposerTextInput id={inputId} aria-label="Project description" value={form.draft.ownerDescription} rows={3}
          placeholder="Describe the project, or record a voice note" disabled={form.submitting || form.transcribing}
          onChange={(event) => form.setDraft((current) => ({ ...current, ownerDescription: event.target.value }))} />
      </ComposerInputShell>
      {!voiceUi.isPreview && <ComposerActionButton voiceUi={voiceUi} sendDisabled={busy} onSend={() => {}} />}
    </div>
    {form.transcribing && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">Transcribing description…</p>}
    {form.error && <p role="alert" className="text-xs text-[var(--ct-red)]">{form.error}</p>}
    <div className="flex justify-end gap-2">
      <button type="button" className={UI_BUTTON.ghost} disabled={form.submitting} onClick={props.onCancel}>Cancel</button>
      <button type="submit" className={UI_BUTTON.primary} disabled={!canStart}>{form.submitting ? "Starting…" : "Start"}</button>
    </div>
  </FormDialog>;
}
