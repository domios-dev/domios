import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useId, useRef, useState } from "react";
import { parseVoiceMessage } from "@codeterm/chat-ui";
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
  reconcileCartographerMapDraft,
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
    setDraft((current) => reconcileCartographerMapDraft(current, providers));
  }, [providers]);
  const transcribe = async (paths: string[]) => {
    if (inFlight.current || paths.length === 0) return;
    setTranscribing(true);
    setError(null);
    try {
      const text = (await Promise.all(paths.map(async (path) => {
        const line = await composerAttachmentHandoff.transcribeLocal(path);
        const transcript = parseVoiceMessage(line)?.transcript;
        if (!transcript) throw new Error("Voice transcription is unavailable. Check your transcriber connection and record again.");
        return transcript;
      }))).join("\n");
      if (mounted.current) setDraft((current) => ({ ...current, ownerDescription: current.ownerDescription ? `${current.ownerDescription}\n${text}` : text }));
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (mounted.current) setTranscribing(false);
    }
  };
  const run = async (action: () => Promise<void>) => {
    if (inFlight.current || transcribing) return;
    inFlight.current = true;
    setSubmitting(true);
    setError(null);
    try {
      await action();
      onCancel();
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      inFlight.current = false;
      if (mounted.current) setSubmitting(false);
    }
  };
  const submit = () => { if (draft.providerId) return run(() => onStart(draft)); };
  return { draft, setDraft, submitting, transcribing, error, setError, transcribe, submit, run };
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
    {props.catalogueError && <InlineErrorNotice error={props.catalogueError} operation="starting the Cartographer" blocking />}
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
      onClick={() => { if (props.onAskOrchestrator) void form.run(props.onAskOrchestrator); }}>Ask the Orchestrator</button>}
    <label htmlFor={inputId} className="text-xs font-medium">What is this project about? (optional)</label>
    <div className="flex items-end gap-2">
      <ComposerInputShell voiceUi={voiceUi}>
        <ComposerTextInput id={inputId} aria-label="Project description" value={form.draft.ownerDescription} rows={3}
          placeholder="Describe the project, or record a voice note" disabled={form.submitting || form.transcribing}
          onChange={(event) => {
            const ownerDescription = event.target.value;
            form.setDraft((current) => ({ ...current, ownerDescription }));
          }} />
      </ComposerInputShell>
      {!voiceUi.isPreview && <ComposerActionButton voiceUi={voiceUi} sendDisabled={busy} onSend={() => {}} />}
    </div>
    {form.transcribing && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">Transcribing description…</p>}
    {form.error && <InlineErrorNotice error={form.error} operation="starting the Cartographer" blocking />}
    <div className="flex justify-end gap-2">
      <button type="button" className={UI_BUTTON.ghost} disabled={form.submitting} onClick={props.onCancel}>Cancel</button>
      <button type="submit" className={UI_BUTTON.primary} disabled={!canStart}>{form.submitting ? "Starting…" : "Start"}</button>
    </div>
  </FormDialog>;
}
