import { useEffect, useState } from "react";
import { Undo2 } from "lucide-react";
import type { AppearanceProposal } from "../../lib/appearance";
import { APPEARANCE_CHANGED, appearanceError, changeAppearance, readAppearance, type AppearanceTarget } from "../../lib/appearanceClient";
import { Button } from "../shared/Button";

export function AppearanceLabUndo({ peer, disabled }: { peer?: string; disabled?: boolean }) {
  const [active, setActive] = useState<AppearanceProposal>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true;
    let receivedChange = false;
    const onChange = (event: Event) => {
      const { target, receipt } = (event as CustomEvent<{ target: AppearanceTarget; receipt: AppearanceProposal }>).detail;
      if (peer !== target.peer) return;
      receivedChange = true;
      setActive(receipt.can_undo ? receipt : undefined);
    };
    window.addEventListener(APPEARANCE_CHANGED, onChange);
    void readAppearance({ machine: peer ?? "local", peer }).then(current => {
      if (live && !receivedChange) setActive(current.active ?? undefined);
    }).catch(() => {});
    return () => {
      live = false;
      window.removeEventListener(APPEARANCE_CHANGED, onChange);
    };
  }, [peer]);

  if (!active?.can_undo) return null;
  async function undo() {
    if (!active || busy || disabled) return;
    setBusy(true);
    setError("");
    try {
      const target = { machine: peer ?? "local", peer };
      await changeAppearance(target, active.id, "undo");
      setActive(latest => latest?.id === active.id ? undefined : latest);
      const current = await readAppearance(target).catch(() => undefined);
      if (current?.active?.id !== active.id) {
        setActive(latest => latest ?? current?.active ?? undefined);
      }
    } catch (failure) {
      setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  }
  return <div>
    {active.theme_label && <p role="status">
      Applied {active.theme_label}
      {active.wallpaper_switched && " · wallpaper switched to the theme's for readability"}
    </p>}
    <Button variant="outline" disabled={busy || disabled} onClick={() => void undo()}>
      <Undo2 size={15} aria-hidden="true" /> Undo · back to {active.previous.theme.name}
    </Button>
    {error && <p role="alert">{error}</p>}
  </div>;
}
