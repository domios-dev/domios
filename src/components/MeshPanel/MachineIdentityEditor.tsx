import { useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { PageDataContext } from "../../hooks/PageDataContext";
import { usePhoneViewport } from "../../hooks/useSidebarViewport";
import { useBackDismissal } from "../../lib/backDismissal";
import { X } from "lucide-react";
import { cleanMachineName, MACHINE_KINDS, type MachineIdentity } from "../../lib/machineIdentity";
import { readMachineIdentity, saveMachineIdentity, type MachineIdentityView } from "../../lib/machineIdentityClient";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { AgentIcon } from "../AgentIcon";
import { Button } from "../shared/Button";
import { MachineGlyph, MachineIconBadge } from "../shared/MachineGlyph";
import { machineColourLabel, MachineIdentityControls, MachineIdentitySaveError } from "./MachineIdentityControls";

export function useMachineJoin(enabled: boolean, peerId?: string) {
  const [view, setView] = useState<MachineIdentityView>();
  const [error, setError] = useState<string>();
  const [draft, setDraft] = useState<MachineIdentity>();
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    void readMachineIdentity(peerId).then(result => {
      if (live) setView(result);
    }).catch(() => undefined);
    return () => { live = false; };
  }, [enabled, peerId]);

  async function beforeJoin() {
    if (!view || view.chosen) return true;
    try {
      const identity = await saveMachineIdentity(draft ?? view.identity, peerId);
      setView({ ...view, identity, chosen: true });
      setError(undefined);
      return true;
    } catch (error) {
      setError(String(error));
      return false;
    }
  }

  return { view: view && !view.chosen ? view : undefined, value: draft ?? view?.identity, setDraft, beforeJoin, error };
}

export interface MachineRename {
  displayName?: string | null;
  save: (name: string) => Promise<boolean>;
}

export function MachineIdentityEditor({ name, value, setValue, error, retry, onClose, phone, nameField }: {
  name: string;
  value: MachineIdentity;
  setValue: (identity: MachineIdentity) => void;
  error?: string;
  retry: () => void;
  onClose: () => void;
  phone: boolean;
  nameField?: React.ReactNode;
}) {
  const providerId = useContext(PageDataContext)?.config.config.general_agent.agent_provider;
  const displayName = cleanMachineName(name) ?? "This machine";
  return <div role="group" aria-label="Icon and colour" className="machine-identity-editor">
    {phone && <span className="machine-identity-editor__handle" aria-hidden="true" />}
    <div className="flex items-center justify-between text-[length:var(--ct-font-size-md)] font-semibold">
      Icon and colour
      {!phone && <Button size="compact" variant="icon" aria-label="Close" onClick={onClose}><X size={14} /></Button>}
    </div>
    <div className="machine-identity-editor__preview" aria-hidden="true">
      <div className="flex min-w-0 items-center gap-2.5">
        <MachineGlyph {...value} size="tile" />
        <div className="min-w-0">
          <div className="truncate font-semibold" title={displayName}>{displayName}</div>
          <div className="machine-identity-editor__meta">{MACHINE_KINDS[value.kind].label} · {machineColourLabel(value.color)}</div>
        </div>
      </div>
      <div className="border-l border-[var(--ct-border)] pl-3">
        <div className="machine-identity-editor__meta mb-1">On other machines</div>
        <div className="machine-identity-editor__sidebar">
          <span className="sidebar-remote-tab-icon">
            <AgentIcon providerId={providerId} className="size-3" />
            <MachineIconBadge identity={value} />
          </span>
          <span><span className="block font-semibold">Main</span><span className="machine-identity-editor__meta">General Agent</span></span>
        </div>
      </div>
    </div>
    {nameField}
    <MachineIdentityControls value={value} onChange={setValue} />
    {error && <MachineIdentitySaveError error={error} retry={retry} />}
    {phone && <Button className="machine-identity-editor__done" onClick={onClose}>Done</Button>}
  </div>;
}

export function MachineIdentityPopover({ name, peerId, initial, onClose, anchor, rename, focusName }: {
  name: string;
  peerId?: string;
  initial: MachineIdentity;
  onClose: () => void;
  anchor: React.RefObject<HTMLElement | null>;
  rename?: MachineRename;
  focusName?: boolean;
}) {
  const [value, setValue] = useState(initial);
  const [failure, setFailure] = useState<string>();
  const baseName = cleanMachineName(rename?.displayName);
  const [draft, setDraft] = useState<string>();
  const [nameError, setNameError] = useState<string>();
  const savedName = useRef(baseName);
  const namePending = useRef(false);
  const [savingName, setSavingName] = useState(false);
  const previewName = draft?.trim() && draft.trim() !== baseName ? draft.trim() : name;
  async function saveName() {
    if (!rename || draft === undefined || namePending.current) return;
    const next = cleanMachineName(draft);
    if (!next || new TextEncoder().encode(next).length > 128 || [...next].some(char => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127)) {
      setNameError("Enter a machine name up to 128 bytes without control characters.");
      return;
    }
    if (next === (savedName.current ?? baseName)) {
      setNameError(undefined);
      return;
    }
    namePending.current = true;
    setSavingName(true);
    try {
      if (!await rename.save(next)) throw new Error("The machine is unavailable.");
      savedName.current = next;
      setDraft(current => current === draft ? next : current);
      setNameError(undefined);
    } catch (error) {
      setNameError(error instanceof Error ? error.message : String(error));
    } finally {
      namePending.current = false;
      setSavingName(false);
    }
  }
  const pending = useRef(Promise.resolve());
  const sequence = useRef(0);
  const editor = useRef<HTMLDivElement | null>(null);
  const phone = usePhoneViewport();
  useBackDismissal(true, onClose);
  useLayoutEffect(() => {
    editor.current?.querySelector<HTMLElement>(focusName ? "[data-machine-name]" : '[role="radio"][aria-checked="true"]')?.focus({ preventScroll: true });
  }, [focusName]);
  useEffect(() => {
    let live = true;
    void readMachineIdentity(peerId).then(result => {
      if (live && sequence.current === 0) setValue(result.identity);
    }).catch(() => undefined);
    return () => { live = false; };
  }, [peerId]);
  function update(next: MachineIdentity) {
    const revision = ++sequence.current;
    setValue(next);
    setFailure(undefined);
    pending.current = pending.current.then(async () => {
      if (sequence.current !== revision) return;
      try {
        await saveMachineIdentity(next, peerId);
      } catch (error) {
        if (sequence.current === revision) setFailure(String(error));
      }
    });
  }

  return <AnchoredPopover open anchor={anchor} placement="bottom-end" bottomSheet={phone} className={phone ? "machine-identity-sheet" : undefined} onClose={onClose}>
    <div ref={editor} className="ct-menu ct-menu--opaque machine-identity-menu">
      <MachineIdentityEditor name={previewName} value={value} setValue={update} error={failure} retry={() => update(value)} onClose={onClose} phone={phone}
        nameField={rename && <>
          <label className="grid gap-1 text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
            Machine name
            <input className="ct-field text-[var(--ct-fg)]" data-machine-name value={draft ?? baseName ?? ""} readOnly={savingName} aria-busy={savingName || undefined}
              onChange={event => { setDraft(event.target.value); setNameError(undefined); }}
              onBlur={() => void saveName()} maxLength={128}
              onKeyDown={event => {
                if (event.key !== "Enter") return;
                event.preventDefault();
                void saveName();
              }} />
          </label>
          {nameError && <MachineIdentitySaveError title="Couldn't save the machine name." error={nameError} retry={() => void saveName()} />}
        </>} />
    </div>
  </AnchoredPopover>;
}
