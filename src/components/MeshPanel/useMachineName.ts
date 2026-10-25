import { useRef, useState } from "react";

export function useMachineName(name: string, onRename?: (name: string) => Promise<boolean>) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const [saving, setSaving] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const active = useRef(false);
  const pending = useRef(false);
  function cancel() {
    active.current = false;
    setEditing(false);
  }
  function begin() {
    active.current = true;
    setDraft(name);
    setFailed(null);
    setEditing(true);
  }
  async function save() {
    const next = draft.trim();
    if (!active.current || pending.current || !next || !onRename) return;
    if (next === name) { cancel(); return; }
    pending.current = true;
    setSaving(true);
    setFailed(null);
    try {
      if (await onRename(next)) cancel();
      else setFailed("Could not rename. Try again.");
    } catch (error) {
      setFailed(error instanceof Error ? error.message : "Could not rename. Try again.");
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }
  return { editing, draft, setDraft, saving, failed, begin, cancel, save };
}
