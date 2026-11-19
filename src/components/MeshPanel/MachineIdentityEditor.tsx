import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { type MachineIdentity } from "../../lib/machineIdentity";
import { readMachineIdentity, saveMachineIdentity, type MachineIdentityView } from "../../lib/machineIdentityClient";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineIdentityControls, MachineIdentitySaveError } from "./MachineIdentityControls";

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

export function MachineIdentityEditor({ name, value, setValue, error, retry }: {
  name: string;
  value: MachineIdentity;
  setValue: (identity: MachineIdentity) => void;
  error?: string;
  retry: () => void;
}) {
  return <div role="group" aria-label="Icon and colour" className="grid gap-3 p-3">
    <div className="flex min-w-0 items-center gap-2">
      <MachineGlyph {...value} size="tile" />
      <div className="min-w-0">
        <div className="font-medium">Icon and colour</div>
        <div className="truncate text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]" title={name}>{name}</div>
      </div>
    </div>
    <MachineIdentityControls value={value} onChange={setValue} />
    {error && <MachineIdentitySaveError error={error} retry={retry} />}
  </div>;
}

export function MachineIdentityPopover({ name, peerId, initial, onClose, anchor }: {
  name: string;
  peerId?: string;
  initial: MachineIdentity;
  onClose: () => void;
  anchor: React.RefObject<HTMLElement | null>;
}) {
  const [value, setValue] = useState(initial);
  const [failure, setFailure] = useState<string>();
  const pending = useRef(Promise.resolve());
  const sequence = useRef(0);
  const editor = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    editor.current?.querySelector<HTMLButtonElement>('[role="radio"][aria-checked="true"]')?.focus({ preventScroll: true });
  }, []);
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

  return <AnchoredPopover open anchor={anchor} placement="bottom-end" onClose={onClose}>
    <div ref={editor} className="ct-menu w-[min(340px,calc(100vw-24px))]">
      <MachineIdentityEditor name={name} value={value} setValue={update} error={failure} retry={() => update(value)} />
    </div>
  </AnchoredPopover>;
}
