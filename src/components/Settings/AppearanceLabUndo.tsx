import { useEffect, useState } from "react";
import { Undo2 } from "lucide-react";
import type { AppearanceProposal } from "../../lib/appearance";
import { appearanceError, changeAppearance, readAppearance } from "../../lib/appearanceClient";
import { Button } from "../shared/Button";

export function AppearanceLabUndo({ peer }: { peer?: string }) {
  const [active, setActive] = useState<AppearanceProposal>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true;
    void readAppearance({ machine: peer ?? "local", peer }).then(current => {
      if (live) setActive(current.active ?? undefined);
    }).catch(() => {});
    return () => { live = false; };
  }, [peer]);

  if (!active?.can_undo) return null;
  async function undo() {
    if (!active || busy) return;
    setBusy(true);
    setError("");
    try {
      await changeAppearance({ machine: peer ?? "local", peer }, active.id, "undo");
      setActive(undefined);
    } catch (failure) {
      setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  }
  return <div>
    <Button variant="outline" disabled={busy} onClick={() => void undo()}>
      <Undo2 size={15} aria-hidden="true" /> Undo · back to {active.previous.theme.name}
    </Button>
    {error && <p role="alert">{error}</p>}
  </div>;
}
